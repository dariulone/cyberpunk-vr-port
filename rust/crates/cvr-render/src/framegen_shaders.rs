//! The frame generator's own passes as HLSL (framegen_generator.rs): the reference the DXIL made from Rust
//! (rust/shaders/passes/src/framegen.rs) replaces, kept for CVR_SHADER_HLSL and the WARP bench below.

/// Pack, Flow, SceneCut: the colour packed to the generator's size, the motion flow, the scene-cut test.
pub(crate) const SHADER: &str = r#"
Texture2D<float4> color : register(t0);
Texture2D<int2> rawFlow : register(t1);
Texture2D<float4> previous : register(t2);
RWTexture2D<float4> packed : register(u0);
RWTexture2D<int2> flow : register(u1);
RWTexture2D<uint> scd : register(u2);
SamplerState linearClamp : register(s0);
cbuffer Params : register(b0) { uint sw,sh,dw,dh,reset,clearScd; }
[numthreads(8,8,1)] void Pack(uint3 id:SV_DispatchThreadID) {
    if(id.x<dw && id.y<dh) packed[id.xy]=float4(color.SampleLevel(linearClamp,(id.xy+.5)/float2(dw,dh),0).rgb,1);
}
[numthreads(8,8,1)] void Flow(uint3 id:SV_DispatchThreadID) {
    if(id.x<dw && id.y<dh) {
        uint2 p=min(uint2((id.xy+.5)*float2(sw,sh)/float2(dw,dh)),uint2(sw-1,sh-1));
        flow[id.xy]=reset ? int2(0,0) : rawFlow.Load(int3(p,0));
    }
    if(clearScd!=0 && id.x==0 && id.y==0) {scd[uint2(0,0)]=0;scd[uint2(1,0)]=0;scd[uint2(2,0)]=0;}
}
groupshared float difference[64];
[numthreads(8,8,1)] void SceneCut(uint3 id:SV_GroupThreadID,uint index:SV_GroupIndex) {
    float2 uv=(id.xy+.5)/8;
    float3 a=color.SampleLevel(linearClamp,uv,0).rgb,b=previous.SampleLevel(linearClamp,uv,0).rgb;
    difference[index]=dot(abs(a-b),float3(.2126,.7152,.0722));GroupMemoryBarrierWithGroupSync();
    for(uint step=32;step;step/=2) { if(index<step) difference[index]+=difference[index+step];GroupMemoryBarrierWithGroupSync(); }
    if(index==0) { uint cut=reset || difference[0]/64>.45 ? 15 : 0;scd[uint2(0,0)]=cut;scd[uint2(1,0)]=cut;scd[uint2(2,0)]=cut; }
}
"#;
/// PrepareInputs: the depth and motion at the generator's size.
pub(crate) const PREPARE_INPUTS_SHADER: &str = r#"
Texture2D<float> sourceDepth : register(t0);
Texture2D<float4> sourceMotion : register(t1);
RWTexture2D<float> targetDepth : register(u0);
RWTexture2D<float2> targetMotion : register(u1);
cbuffer Params : register(b0) { uint sw,sh,dw,dh,inverted,padding; }
[numthreads(8,8,1)] void PrepareInputs(uint3 id:SV_DispatchThreadID) {
    if(id.x>=dw || id.y>=dh)return;
    float2 pixel=(id.xy+.5)*float2(sw,sh)/float2(dw,dh);
    int2 chosen=min(int2(pixel),int2(sw-1,sh-1));
    float depth=sourceDepth.Load(int3(chosen,0));
    if(sw>dw || sh>dh) {
        int2 base=int2(floor(pixel-.5));
        [unroll] for(int y=0;y<2;++y) [unroll] for(int x=0;x<2;++x) {
            int2 p=clamp(base+int2(x,y),int2(0,0),int2(sw-1,sh-1));
            float z=sourceDepth.Load(int3(p,0));
            if(inverted ? z>depth : z<depth) {depth=z;chosen=p;}
        }
    }
    uint mw,mh;sourceMotion.GetDimensions(mw,mh);
    uint2 motionPixel=min(uint2((chosen+.5)*float2(mw,mh)/float2(sw,sh)),uint2(mw-1,mh-1));
    targetDepth[id.xy]=depth;
    targetMotion[id.xy]=sourceMotion.Load(int3(motionPixel,0)).xy;
}
"#;

#[cfg(test)]
mod shader_parity;
