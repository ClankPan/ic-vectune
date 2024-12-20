use vectune::PointInterface;

// #[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
#[derive(candid::CandidType, candid::Deserialize, Clone, Debug)]
pub struct Point(Vec<f32>);

impl PointInterface for Point {
    fn distance(&self, other: &Self) -> f32 {
        // -cosine_similarity(&self, &other) + 1.0
        1.0 - dot_product(self, other) //  use dot production instead of con-sim because vector is already normalized.
    }

    fn add(&self, other: &Self) -> Self {
        Point::from_f32_vec(
            self.to_f32_vec()
                .into_iter()
                .zip(other.to_f32_vec())
                .map(|(x, y)| x + y)
                .collect(),
        )
    }
    fn div(&self, divisor: &usize) -> Self {
        Point::from_f32_vec(
            self.to_f32_vec()
                .into_iter()
                .map(|v| v / *divisor as f32)
                .collect(),
        )
    }

    fn to_f32_vec(&self) -> Vec<f32> {
        self.0.iter().copied().collect()
    }
    fn from_f32_vec(a: Vec<f32>) -> Self {
        Point(a.into_iter().collect())
    }
}


#[cfg(target_arch = "wasm32")]
#[target_feature(enable = "simd128")]
fn dot_product(vec1: &Point, vec2: &Point) -> f32 {
    use std::arch::wasm32::*;

    assert_eq!(vec1.0.len(), vec2.0.len());
    // let dim: usize = vec1.0.len();
    // let mut result = 0.0;
    // for i in 0..dim {
    //     result += vec1.0[i] * vec2.0[i];
    // }
    // result

    let mut result = f32x4_splat(0.0);

    vec1.0.chunks(4).zip(vec2.0.chunks(4)).for_each(|(a, b)| {
        let (a, b) = if a.len() < 4 {
            let mut padded_a = [0.0f32; 4];
            let mut padded_b = [0.0f32; 4];
            padded_a[..a.len()].copy_from_slice(a);
            padded_b[..b.len()].copy_from_slice(b);

            let a = unsafe { v128_load(padded_a.as_ptr() as *const v128) };
            let b = unsafe { v128_load(padded_a.as_ptr() as *const v128) };

            (a, b)
        } else {
            let a = unsafe { v128_load(a.as_ptr() as *const v128) };
            let b = unsafe { v128_load(b.as_ptr() as *const v128) };
            (a, b)
        };
        let mul = f32x4_mul(a, b);
        result = f32x4_add(result, mul);
    });

    let final_result = f32x4_extract_lane::<0>(result)
        + f32x4_extract_lane::<1>(result)
        + f32x4_extract_lane::<2>(result)
        + f32x4_extract_lane::<3>(result);

    final_result
}


#[cfg(not(target_arch = "wasm32"))]
fn dot_product(_vec1: &Point, _vec2: &Point) -> f32 {
    todo!();
}
