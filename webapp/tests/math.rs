#[path = "../src/math.rs"]
mod math;

#[test]
fn supersampled_math_retains_partial_coverage() {
    let image = math::rasterize(r"x^2+y^2=25", true, 1.0).expect("RaTeX image");
    assert!(image.width > 20 && image.height > 8);
    assert!(
        image
            .pixels
            .chunks_exact(4)
            .any(|pixel| pixel[3] > 0 && pixel[3] < 255)
    );
}
