//! Which pipeline state stream subobjects carry shader bytecode (src/Render/ViewInstancedPipeline.cpp, `IsShaderPart`:
//! the vertex, pixel, domain, hull, geometry, compute, amplification and mesh shaders), the parts a pipeline stream's
//! owned copy duplicates and counts.
//!
//! The .cpp's IsShaderPart forwards to `cvr_vip_is_shader_part`; the unit test below checks every subobject type.
use windows::Win32::Graphics::Direct3D12::{
    D3D12_PIPELINE_STATE_SUBOBJECT_TYPE, D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS, D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CS,
    D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DS, D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_GS, D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_HS,
    D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS, D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS, D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS,
};

/// Whether a subobject of this type is a D3D12_SHADER_BYTECODE.
pub fn is_shader_part(kind: i32) -> bool {
    const SHADERS: [D3D12_PIPELINE_STATE_SUBOBJECT_TYPE; 8] = [
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_VS,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_PS,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_DS,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_HS,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_GS,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_CS,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_AS,
        D3D12_PIPELINE_STATE_SUBOBJECT_TYPE_MS,
    ];
    SHADERS.iter().any(|s| s.0 == kind)
}
#[no_mangle]
pub extern "C-unwind" fn cvr_vip_is_shader_part(kind: i32) -> bool {
    is_shader_part(kind)
}

#[cfg(test)]
mod tests {
    //! Every subobject type of d3d12.h.
    use super::*;

    #[test]
    fn shader_parts() {
        // d3d12.h: root signature 0, VS 1, PS 2, DS 3, HS 4, GS 5, CS 6, stream output 7 ... view instancing 22,
        // AS 24, MS 25 (the last, 26, is MAX_VALID).
        let shaders: Vec<i32> = (-1..=27).filter(|&k| is_shader_part(k)).collect();
        assert_eq!(shaders, [1, 2, 3, 4, 5, 6, 24, 25]);
    }
}
