use rand::prelude::*;

pub fn project_to_2d(embedding: &[f32]) -> (f32, f32) {
    if embedding.is_empty() {
        return (0.0, 0.0);
    }

    let x = embedding
        .iter()
        .step_by(2)
        .enumerate()
        .map(|(index, value)| *value * ((index + 1) as f32).sin())
        .sum::<f32>();
    let y = embedding
        .iter()
        .skip(1)
        .step_by(2)
        .enumerate()
        .map(|(index, value)| *value * ((index + 1) as f32).cos())
        .sum::<f32>();

    (x, y)
}
