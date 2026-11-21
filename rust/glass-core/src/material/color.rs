//! Recovered CoreAnimation 4×5 material color matrices.
use crate::Result;

type Matrix = [[f32; 5]; 4];

// Original bases in shaders/data/material-completion-machine.json.
const TO_YCC: Matrix = [
    [0.2126, 0.7152, 0.0722, 0.0, 0.0],
    [-0.1146, -0.3854, 0.5, 0.0, 0.5],
    [0.5, -0.4542, -0.0458, 0.0, 0.5],
    [0.0, 0.0, 0.0, 1.0, 0.0],
];
const FROM_YCC: Matrix = [
    [1.0, 0.0, 1.5748, 0.0, -0.7874],
    [1.0, -0.187324, -0.468124, 0.0, 0.327724],
    [1.0, 1.8556, 0.0, 0.0, -0.9278],
    [0.0, 0.0, 0.0, 1.0, 0.0],
];

fn concatenate(a: Matrix, b: Matrix) -> Matrix {
    // Operand accumulation order varies in the original 26A434 function.
    const REGULAR: [usize; 4] = [3, 2, 1, 0];
    const INNER: [[usize; 4]; 16] = [
        REGULAR, REGULAR, REGULAR, REGULAR,
        REGULAR, REGULAR, REGULAR, [3, 1, 2, 0],
        REGULAR, REGULAR, [3, 2, 0, 1], [3, 0, 2, 1],
        REGULAR, [3, 1, 2, 0], [3, 0, 2, 1], [3, 0, 1, 2],
    ];
    const TRANSLATION: [[usize; 4]; 4] =
        [REGULAR, [2, 1, 3, 0], [2, 0, 3, 1], [0, 1, 3, 2]];

    std::array::from_fn(|row| {
        std::array::from_fn(|col| {
            if col == 4 {
                return TRANSLATION[row].into_iter().fold(a[row][4], |acc, k| {
                    a[row][k].mul_add(b[k][col], acc)
                });
            }
            let [first, rest @ ..] = INNER[row * 4 + col];
            rest.into_iter().fold(a[row][first] * b[first][col], |acc, k| {
                a[row][k].mul_add(b[k][col], acc)
            })
        })
    })
}

/// Explicit working-space parameters for CA's set_ycc_composite.
/// The fill has already been converted to linear premultiplied RGBA.
pub struct YccColor {
    pub white: f32,
    pub black: f32,
    pub saturation: f32,
    pub fill: [f32; 4],
}

impl YccColor {
    pub fn matrix(&self) -> Result<Matrix> {
        if ![self.white, self.black, self.saturation].into_iter().chain(self.fill).all(f32::is_finite) {
            return Err("finite YCC matrix inputs required".into());
        }
        let contrast = [
            [self.white-self.black, 0.0, 0.0, 0.0, self.black],
            [0.0, 1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0, 0.0],
        ];
        let chroma_offset = (0.5 - f64::from(self.saturation) * 0.5) as f32;
        let chroma = [
            [1.0, 0.0, 0.0, 0.0, 0.0],
            [0.0, self.saturation, 0.0, 0.0, chroma_offset],
            [0.0, 0.0, self.saturation, 0.0, chroma_offset],
            [0.0, 0.0, 0.0, 1.0, 0.0],
        ];
        let mut result = concatenate(FROM_YCC, concatenate(chroma, concatenate(contrast, TO_YCC)));
        let factor = 1.0 - self.fill[3];
        for value in result.iter_mut().flatten() { *value *= factor; }
        for (row, value) in result[..3].iter_mut().zip(self.fill) { row[4] += value; }
        result[3][3] += self.fill[3];
        if !result.iter().flatten().all(|v| v.is_finite()) {
            return Err("YCC matrix overflow".into());
        }
        Ok(result)
    }
}

/// Linear source-over used by the demo's explicit Clear face policy.
pub fn linear_fill_matrix([red, green, blue, alpha]: [f32; 4]) -> Matrix {
    let factor = 1.0 - alpha;
    [
        [factor, 0.0, 0.0, 0.0, red],
        [0.0, factor, 0.0, 0.0, green],
        [0.0, 0.0, factor, 0.0, blue],
        [0.0, 0.0, 0.0, 1.0, 0.0],
    ]
}
