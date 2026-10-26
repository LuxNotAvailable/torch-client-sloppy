use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::log_warn;

const ATTRIBUTES_URL: &str = "https://api.minecraftservices.com/player/attributes";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BanDetails {
    pub id: Option<String>,
    pub expires: Option<i64>,
    pub reason: Option<String>,
    pub reason_message: Option<String>,
}

struct State {
    generation: u64,
    checked: Option<u64>,
    ban: Option<BanDetails>,
    shown: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    generation: 0,
    checked: None,
    ban: None,
    shown: false,
});

static JOIN_REFUSED: AtomicBool = AtomicBool::new(false);

pub(crate) fn check_once() {
    let generation = {
        let mut state = STATE.lock().unwrap();
        if state.checked == Some(state.generation) {
            return;
        }
        state.checked = Some(state.generation);
        state.generation
    };
    crate::platform::executor::spawn_detached(async move {
        let ban = match check().await {
            Ok(ban) => ban,
            Err(e) => {
                log_warn!("auth", "could not check for a multiplayer ban: {e}");
                None
            }
        };
        if let Some(ban) = &ban {
            log_warn!("auth", "this account is banned from multiplayer: {ban:?}");
        }
        let mut state = STATE.lock().unwrap();
        if state.generation == generation {
            state.ban = ban;
        }
    });
}

pub(crate) fn account_changed() {
    let mut state = STATE.lock().unwrap();
    state.generation += 1;
    state.ban = None;
    state.shown = false;
}

pub(crate) fn take_unseen() -> bool {
    let mut state = STATE.lock().unwrap();
    if state.ban.is_none() || state.shown {
        return false;
    }
    state.shown = true;
    true
}

pub(crate) fn current() -> Option<BanDetails> {
    STATE.lock().unwrap().ban.clone()
}

pub(crate) fn join_refused() {
    JOIN_REFUSED.store(true, Ordering::Relaxed);
}

pub(crate) fn take_join_refused() -> bool {
    JOIN_REFUSED.swap(false, Ordering::Relaxed)
}

async fn check() -> Result<Option<BanDetails>, String> {
    #[cfg(not(target_arch = "wasm32"))]
    if let Ok(body) = std::env::var("MC_FAKE_BAN_ATTRIBUTES") {
        log_warn!(
            "auth",
            "MC_FAKE_BAN_ATTRIBUTES is set; using it as the ban check's reply"
        );
        return parse_attributes(&body);
    }
    let Some(session) = crate::client::auth::active_session().await? else {
        return Ok(None);
    };
    let response = crate::client::auth::client()
        .get(ATTRIBUTES_URL)
        .bearer_auth(&session.token)
        .send()
        .await
        .and_then(|r| r.error_for_status())
        .map_err(|e| e.to_string())?;
    let body = response.text().await.map_err(|e| e.to_string())?;
    parse_attributes(&body)
}

fn parse_attributes(body: &str) -> Result<Option<BanDetails>, String> {
    let json: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    let Some(scope) = json
        .pointer("/banStatus/bannedScopes/MULTIPLAYER")
        .filter(|v| v.is_object())
    else {
        return Ok(None);
    };
    let text = |key: &str| scope.get(key).and_then(|v| v.as_str()).map(str::to_string);
    let expires = match scope.get("expires") {
        None | Some(serde_json::Value::Null) => None,
        Some(v) => Some(
            v.as_str()
                .and_then(parse_instant)
                .ok_or_else(|| format!("unreadable ban expiry {v}"))?,
        ),
    };
    Ok(Some(BanDetails {
        id: text("banId"),
        expires,
        reason: text("reason"),
        reason_message: text("reasonMessage"),
    }))
}

fn parse_instant(s: &str) -> Option<i64> {
    fn digits(s: Option<&str>) -> Option<i64> {
        let s = s?;
        (!s.is_empty() && s.bytes().all(|c| c.is_ascii_digit())).then(|| s.parse().ok())?
    }
    let b = s.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || !matches!(b[10], b'T' | b't')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let (year, month, day) = (
        digits(s.get(0..4))?,
        digits(s.get(5..7))?,
        digits(s.get(8..10))?,
    );
    let (hour, minute, second) = (
        digits(s.get(11..13))?,
        digits(s.get(14..16))?,
        digits(s.get(17..19))?,
    );
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    let mut rest = s.get(19..)?;
    if let Some(fraction) = rest.strip_prefix('.') {
        let n = fraction.bytes().take_while(u8::is_ascii_digit).count();
        if n == 0 {
            return None;
        }
        rest = &fraction[n..];
    }
    let offset = match rest {
        "Z" | "z" => 0,
        _ if rest.len() == 6 && rest.as_bytes()[3] == b':' => {
            let sign = match rest.as_bytes()[0] {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            sign * (digits(rest.get(1..3))? * 3600 + digits(rest.get(4..6))? * 60)
        }
        _ => return None,
    };
    Some(days_from_civil(year, month, day) * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year - era * 400;
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instants() {
        assert_eq!(parse_instant("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_instant("2024-03-01T12:00:00Z"), Some(1_709_294_400));
        assert_eq!(
            parse_instant("2024-03-01T12:00:00.123456Z"),
            Some(1_709_294_400)
        );
        assert_eq!(
            parse_instant("2024-03-01T14:00:00+02:00"),
            Some(1_709_294_400)
        );
        assert_eq!(parse_instant("2024-02-29T00:00:00Z"), Some(1_709_164_800));
        assert_eq!(parse_instant("2024-03-01 12:00:00Z"), None);
        assert_eq!(parse_instant("2024-03-01T12:00:00"), None);
        assert_eq!(parse_instant("1709294400"), None);
    }

    #[test]
    fn attributes() {
        let banned = r#"{"privileges":{},"banStatus":{"bannedScopes":{"MULTIPLAYER":{
            "banId":"0a1b2c3d4e5f60718293a4b5c6d7e8f9","expires":"2024-03-01T12:00:00Z",
            "reason":"21","reasonMessage":"text"}}}}"#;
        assert_eq!(
            parse_attributes(banned),
            Ok(Some(BanDetails {
                id: Some("0a1b2c3d4e5f60718293a4b5c6d7e8f9".into()),
                expires: Some(1_709_294_400),
                reason: Some("21".into()),
                reason_message: Some("text".into()),
            }))
        );
        let permanent =
            r#"{"banStatus":{"bannedScopes":{"MULTIPLAYER":{"banId":"x","expires":null}}}}"#;
        assert_eq!(parse_attributes(permanent).unwrap().unwrap().expires, None);
        assert_eq!(
            parse_attributes(r#"{"banStatus":{"bannedScopes":{}}}"#),
            Ok(None)
        );
        assert_eq!(parse_attributes(r#"{"privileges":{}}"#), Ok(None));
        assert!(
            parse_attributes(
                r#"{"banStatus":{"bannedScopes":{"MULTIPLAYER":{"expires":"soon"}}}}"#
            )
            .is_err()
        );
    }
}
