//! What several passes share.
use spirv_std::glam::{Vec2, Vec3};
#[cfg(target_arch = "spirv")]
use spirv_std::num_traits::Float;

/// HLSL's dot(a, b): one OpDot (DXIL's Dot3, FXC's dp3). glam's `dot` is written out as three products and two sums,
/// which round apart from the instruction -- the reprojection's colours one step apart on WARP (osr_parity).
#[cfg(target_arch = "spirv")]
pub fn dot3(a: Vec3, b: Vec3) -> f32 {
    let mut result = 0.0f32;
    unsafe {
        core::arch::asm! {
            "%a = OpLoad _ {a}",
            "%b = OpLoad _ {b}",
            "%result = OpDot typeof*{result} %a %b",
            "OpStore {result} %result",
            a = in(reg) &a,
            b = in(reg) &b,
            result = in(reg) &mut result,
        }
    }
    result
}
#[cfg(not(target_arch = "spirv"))]
pub fn dot3(a: Vec3, b: Vec3) -> f32 {
    a.dot(b)
}

/// HLSL's length(v): sqrt(dot(v, v)), as FXC writes it (dp3, sqrt).
pub fn length3(v: Vec3) -> f32 {
    dot3(v, v).sqrt()
}

/// HLSL's dot of two float2: one OpDot (FXC's dp2, DXIL's Dot2) -- `(u * 2) * v` is FXC's dp2((u, u), (v, v)).
#[cfg(target_arch = "spirv")]
pub fn dot2(a: Vec2, b: Vec2) -> f32 {
    let mut result = 0.0f32;
    unsafe {
        core::arch::asm! {
            "%a = OpLoad _ {a}",
            "%b = OpLoad _ {b}",
            "%result = OpDot typeof*{result} %a %b",
            "OpStore {result} %result",
            a = in(reg) &a,
            b = in(reg) &b,
            result = in(reg) &mut result,
        }
    }
    result
}
#[cfg(not(target_arch = "spirv"))]
pub fn dot2(a: Vec2, b: Vec2) -> f32 {
    a.dot(b)
}

/// HLSL's length of a float2: sqrt(dot(v, v)), as DXC and FXC write it.
pub fn length2(v: Vec2) -> f32 {
    dot2(v, v).sqrt()
}

/// HLSL's max / min of two floats themselves (GLSL FMax / FMin, naga's `max` / `min`): rust-gpu writes f32::max and
/// glam's vector max as compare-and-select, which DXC optimises apart from the intrinsic.
#[cfg(target_arch = "spirv")]
pub fn fmax(a: f32, b: f32) -> f32 {
    gl2(a, b, 40)
}
#[cfg(target_arch = "spirv")]
pub fn fmin(a: f32, b: f32) -> f32 {
    gl2(a, b, 37)
}
#[cfg(target_arch = "spirv")]
fn gl2(a: f32, b: f32, op: u32) -> f32 {
    let mut result = 0.0f32;
    // the GLSL.std.450 instruction `op` of two floats (an immediate in the text: asm takes no runtime opcode)
    unsafe {
        if op == 40 {
            core::arch::asm! {
                "%glsl = OpExtInstImport \"GLSL.std.450\"",
                "%a = OpLoad _ {a}",
                "%b = OpLoad _ {b}",
                "%result = OpExtInst typeof*{result} %glsl 40 %a %b",
                "OpStore {result} %result",
                a = in(reg) &a,
                b = in(reg) &b,
                result = in(reg) &mut result,
            }
        } else {
            core::arch::asm! {
                "%glsl = OpExtInstImport \"GLSL.std.450\"",
                "%a = OpLoad _ {a}",
                "%b = OpLoad _ {b}",
                "%result = OpExtInst typeof*{result} %glsl 37 %a %b",
                "OpStore {result} %result",
                a = in(reg) &a,
                b = in(reg) &b,
                result = in(reg) &mut result,
            }
        }
    }
    result
}
#[cfg(not(target_arch = "spirv"))]
pub fn fmax(a: f32, b: f32) -> f32 {
    a.max(b)
}
#[cfg(not(target_arch = "spirv"))]
pub fn fmin(a: f32, b: f32) -> f32 {
    a.min(b)
}
/// Per component fmax / fmin.
pub fn max3(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(fmax(a.x, b.x), fmax(a.y, b.y), fmax(a.z, b.z))
}
pub fn min3(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(fmin(a.x, b.x), fmin(a.y, b.y), fmin(a.z, b.z))
}

/// HLSL's length(v) itself (GLSL Length, naga's `length`), for the shaders DXC compiled before: DXC lowers the
/// intrinsic its own way (a float2's to products and a sum), which a sqrt of a dot does not reproduce.
#[cfg(target_arch = "spirv")]
pub fn length_of2(v: Vec2) -> f32 {
    let mut result = 0.0f32;
    unsafe {
        core::arch::asm! {
            "%glsl = OpExtInstImport \"GLSL.std.450\"",
            "%v = OpLoad _ {v}",
            "%result = OpExtInst typeof*{result} %glsl 66 %v",
            "OpStore {result} %result",
            v = in(reg) &v,
            result = in(reg) &mut result,
        }
    }
    result
}
#[cfg(not(target_arch = "spirv"))]
pub fn length_of2(v: Vec2) -> f32 {
    v.length()
}

/// HLSL's dot of two float4: one OpDot (DXIL's Dot4).
#[cfg(target_arch = "spirv")]
pub fn dot4(a: spirv_std::glam::Vec4, b: spirv_std::glam::Vec4) -> f32 {
    let mut result = 0.0f32;
    unsafe {
        core::arch::asm! {
            "%a = OpLoad _ {a}",
            "%b = OpLoad _ {b}",
            "%result = OpDot typeof*{result} %a %b",
            "OpStore {result} %result",
            a = in(reg) &a,
            b = in(reg) &b,
            result = in(reg) &mut result,
        }
    }
    result
}
#[cfg(not(target_arch = "spirv"))]
pub fn dot4(a: spirv_std::glam::Vec4, b: spirv_std::glam::Vec4) -> f32 {
    a.dot(b)
}

/// HLSL's normalize, length and cross of a float3 themselves (GLSL Normalize 69, Length 66, Cross 68): DXC lowers the
/// intrinsics its own way (normalize by an rsqrt), which the arithmetic written out does not reproduce.
#[cfg(target_arch = "spirv")]
pub fn normalize3(v: Vec3) -> Vec3 {
    let mut result = Vec3::ZERO;
    unsafe {
        core::arch::asm! {
            "%glsl = OpExtInstImport \"GLSL.std.450\"",
            "%v = OpLoad _ {v}",
            "%result = OpExtInst typeof*{result} %glsl 69 %v",
            "OpStore {result} %result",
            v = in(reg) &v,
            result = in(reg) &mut result,
        }
    }
    result
}
#[cfg(target_arch = "spirv")]
pub fn length_of3(v: Vec3) -> f32 {
    let mut result = 0.0f32;
    unsafe {
        core::arch::asm! {
            "%glsl = OpExtInstImport \"GLSL.std.450\"",
            "%v = OpLoad _ {v}",
            "%result = OpExtInst typeof*{result} %glsl 66 %v",
            "OpStore {result} %result",
            v = in(reg) &v,
            result = in(reg) &mut result,
        }
    }
    result
}
#[cfg(target_arch = "spirv")]
pub fn cross3(a: Vec3, b: Vec3) -> Vec3 {
    let mut result = Vec3::ZERO;
    unsafe {
        core::arch::asm! {
            "%glsl = OpExtInstImport \"GLSL.std.450\"",
            "%a = OpLoad _ {a}",
            "%b = OpLoad _ {b}",
            "%result = OpExtInst typeof*{result} %glsl 68 %a %b",
            "OpStore {result} %result",
            a = in(reg) &a,
            b = in(reg) &b,
            result = in(reg) &mut result,
        }
    }
    result
}
#[cfg(not(target_arch = "spirv"))]
pub fn normalize3(v: Vec3) -> Vec3 {
    v.normalize()
}
#[cfg(not(target_arch = "spirv"))]
pub fn length_of3(v: Vec3) -> f32 {
    v.length()
}
#[cfg(not(target_arch = "spirv"))]
pub fn cross3(a: Vec3, b: Vec3) -> Vec3 {
    a.cross(b)
}

/// HLSL's frac itself (GLSL Fract 10, naga's `frac`), for the shaders DXC compiled before: DXC writes the intrinsic as
/// its Frc instruction, which `x - floor(x)` does not reproduce bit for bit.
#[cfg(target_arch = "spirv")]
pub fn fract(x: f32) -> f32 {
    let mut result = 0.0f32;
    unsafe {
        core::arch::asm! {
            "%glsl = OpExtInstImport \"GLSL.std.450\"",
            "%x = OpLoad _ {x}",
            "%result = OpExtInst typeof*{result} %glsl 10 %x",
            "OpStore {result} %result",
            x = in(reg) &x,
            result = in(reg) &mut result,
        }
    }
    result
}
#[cfg(not(target_arch = "spirv"))]
pub fn fract(x: f32) -> f32 {
    x - x.floor()
}

/// a / b kept a division (NoContraction): spirv-opt's folding turns a division by a constant into a multiplication by
/// its reciprocal (`x / 0.6` into `x * 1.6666666`), one rounding apart from the HLSL's division -- for the shaders DXC
/// compiled before.
#[cfg(target_arch = "spirv")]
pub fn div(a: f32, b: f32) -> f32 {
    let mut result = 0.0f32;
    unsafe {
        core::arch::asm! {
            "%a = OpLoad _ {a}",
            "%b = OpLoad _ {b}",
            "%result = OpFDiv typeof*{result} %a %b",
            "OpDecorate %result NoContraction",
            "OpStore {result} %result",
            a = in(reg) &a,
            b = in(reg) &b,
            result = in(reg) &mut result,
        }
    }
    result
}
#[cfg(not(target_arch = "spirv"))]
pub fn div(a: f32, b: f32) -> f32 {
    a / b
}

/// a * b kept a multiplication (NoContraction): spirv-opt folds `(x / y) * y` into `x`, where the HLSL divides and
/// multiplies (the scope panel's `slope * axial`) -- for the shaders DXC compiled before.
#[cfg(target_arch = "spirv")]
pub fn mul(a: f32, b: f32) -> f32 {
    let mut result = 0.0f32;
    unsafe {
        core::arch::asm! {
            "%a = OpLoad _ {a}",
            "%b = OpLoad _ {b}",
            "%result = OpFMul typeof*{result} %a %b",
            "OpDecorate %result NoContraction",
            "OpStore {result} %result",
            a = in(reg) &a,
            b = in(reg) &b,
            result = in(reg) &mut result,
        }
    }
    result
}
#[cfg(not(target_arch = "spirv"))]
pub fn mul(a: f32, b: f32) -> f32 {
    a * b
}
