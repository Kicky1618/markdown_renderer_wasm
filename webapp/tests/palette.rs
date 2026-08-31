#[path = "../src/palette.rs"]
mod palette;

fn css_rgb(css: &str) -> [u8; 3] {
    assert_eq!(css.len(), 7);
    assert!(css.starts_with('#'));
    [
        u8::from_str_radix(&css[1..3], 16).unwrap(),
        u8::from_str_radix(&css[3..5], 16).unwrap(),
        u8::from_str_radix(&css[5..7], 16).unwrap(),
    ]
}

#[test]
fn css_and_gpu_channels_share_exact_byte_values() {
    let colors = [
        palette::BG,
        palette::PANEL,
        palette::FG,
        palette::MUTED,
        palette::CYAN,
        palette::GREEN,
        palette::ORANGE,
        palette::PURPLE,
        palette::BLUE,
        palette::YELLOW,
        palette::OPERATOR,
        palette::SELECTION,
        palette::SEARCH_ACTIVE,
        palette::SEARCH_INACTIVE,
        palette::SCROLLBAR_THUMB,
        palette::THEMATIC_BREAK,
        palette::TABLE_HEADER,
        palette::TABLE_STRIPE,
    ];

    for color in colors {
        let css = css_rgb(color.css);
        let gpu = [
            (color.rgba[0] * 255.0).round() as u8,
            (color.rgba[1] * 255.0).round() as u8,
            (color.rgba[2] * 255.0).round() as u8,
        ];
        assert_eq!(gpu, css, "{} differs between CSS and GPU", color.css);
    }
}

#[test]
fn overlay_alpha_is_shared_with_gpu() {
    assert_eq!(palette::SELECTION.rgba[3], 0.38);
    assert_eq!(palette::SEARCH_ACTIVE.rgba[3], 0.72);
    assert_eq!(palette::SEARCH_INACTIVE.rgba[3], 0.38);
}
