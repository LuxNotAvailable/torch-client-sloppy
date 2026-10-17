#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dye {
    pub name: &'static str,
    pub texture_diffuse: u32,
    pub text_color: u32,
}

const fn dye(name: &'static str, texture_diffuse: u32, text_color: u32) -> Dye {
    Dye {
        name,
        texture_diffuse,
        text_color,
    }
}

pub const DYES: [Dye; 16] = [
    dye("white", 16383998, 16777215),
    dye("orange", 16351261, 16738335),
    dye("magenta", 13061821, 16711935),
    dye("light_blue", 3847130, 10141901),
    dye("yellow", 16701501, 16776960),
    dye("lime", 8439583, 12582656),
    dye("pink", 15961002, 16738740),
    dye("gray", 4673362, 8421504),
    dye("light_gray", 10329495, 13882323),
    dye("cyan", 1481884, 65535),
    dye("purple", 8991416, 10494192),
    dye("blue", 3949738, 255),
    dye("brown", 8606770, 9127187),
    dye("green", 6192150, 65280),
    dye("red", 11546150, 16711680),
    dye("black", 1908001, 0),
];

pub fn by_id(id: usize) -> &'static Dye {
    DYES.get(id).unwrap_or(&DYES[0])
}

pub fn id_by_name(name: &str) -> Option<usize> {
    DYES.iter().position(|dye| dye.name == name)
}

pub fn linear_diffuse(id: usize) -> [f32; 4] {
    crate::util::mth::rgb_to_linear(by_id(id).texture_diffuse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_names_follow_the_enum_order() {
        assert_eq!(id_by_name("red"), Some(14));
        assert_eq!(by_id(14).texture_diffuse, 11546150);
        assert_eq!(by_id(16).name, "white");
        assert_eq!(id_by_name("not a colour"), None);
    }

    #[test]
    fn the_diffuse_column_is_not_the_text_column() {
        assert_eq!(DYES[14].texture_diffuse, 11546150);
        assert_eq!(DYES[14].text_color, 16711680);
    }

    #[test]
    fn a_dye_id_tints_its_own_colour_in_linear_space() {
        use crate::util::mth::srgb_byte_to_linear;
        let black = [29, 29, 33].map(srgb_byte_to_linear);
        assert_eq!(linear_diffuse(15), [black[0], black[1], black[2], 1.0]);
        assert_eq!(linear_diffuse(0)[0], srgb_byte_to_linear(249));
    }
}
