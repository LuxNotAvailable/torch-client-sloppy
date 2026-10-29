//! The sound devices to choose from: speakers for the mixer, microphones for
//! voice chat. A device is kept by name; one that is gone falls back to the
//! system default, as vanilla's `soundDevice` option does.

use std::sync::Mutex;

use rodio::cpal;
use rodio::cpal::traits::{DeviceTrait, HostTrait};

/// The names last listed, refreshed on asking (listing can take a moment,
/// so not every frame).
static OUTPUTS: Mutex<Option<Vec<String>>> = Mutex::new(None);
static INPUTS: Mutex<Option<Vec<String>>> = Mutex::new(None);

fn names(devices: Result<impl Iterator<Item = cpal::Device>, cpal::DevicesError>) -> Vec<String> {
    let mut names: Vec<String> = devices
        .map(|list| list.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default();
    names.dedup();
    names
}

fn listed(cache: &Mutex<Option<Vec<String>>>, refresh: bool, list: impl FnOnce() -> Vec<String>) -> Vec<String> {
    let Ok(mut cache) = cache.lock() else {
        return Vec::new();
    };
    if refresh || cache.is_none() {
        *cache = Some(list());
    }
    cache.clone().unwrap_or_default()
}

/// The speakers' names; `refresh` lists them again.
pub fn outputs(refresh: bool) -> Vec<String> {
    listed(&OUTPUTS, refresh, || names(cpal::default_host().output_devices()))
}

/// The microphones' names; `refresh` lists them again.
pub fn inputs(refresh: bool) -> Vec<String> {
    listed(&INPUTS, refresh, || names(cpal::default_host().input_devices()))
}

/// The speaker called `name`, or the default with none named or that one gone.
pub fn output(name: Option<&str>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    name.and_then(|name| {
        host.output_devices()
            .ok()?
            .find(|d| d.name().is_ok_and(|n| n == name))
    })
    .or_else(|| host.default_output_device())
}

/// The microphone called `name`, or the default with none named or that one gone.
pub fn input(name: Option<&str>) -> Option<cpal::Device> {
    let host = cpal::default_host();
    name.and_then(|name| {
        host.input_devices()
            .ok()?
            .find(|d| d.name().is_ok_and(|n| n == name))
    })
    .or_else(|| host.default_input_device())
}

/// The choice after `current` in `[default, devices...]`, wrapping round:
/// what a click on a device button picks next.
pub fn next(current: Option<&str>, devices: &[String]) -> Option<String> {
    match current.and_then(|c| devices.iter().position(|d| d == c)) {
        None => devices.first().cloned(),
        Some(i) => devices.get(i + 1).cloned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_choice_cycles_through_default_and_every_device() {
        let devices = vec!["A".to_owned(), "B".to_owned()];
        assert_eq!(next(None, &devices).as_deref(), Some("A"));
        assert_eq!(next(Some("A"), &devices).as_deref(), Some("B"));
        assert_eq!(next(Some("B"), &devices), None);
        // A device that went away starts the cycle again.
        assert_eq!(next(Some("gone"), &devices).as_deref(), Some("A"));
        assert_eq!(next(None, &[]), None);
    }
}
