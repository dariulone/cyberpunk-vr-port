//! The desktop mirror's present conversion as HLSL (mirror.rs): the reference the DXIL made from Rust
//! (rust/shaders/passes/src/mirror.rs) replaces, kept for CVR_SHADER_HLSL and the WARP bench below.

/// The full-screen triangle and the sRGB encode with its dither.
pub(crate) const CONVERT_HLSL: &str = concat!(
    "Texture2D gTex:register(t0);",
    "cbuffer T:register(b0){float gT;};",
    "void VSMain(uint vid:SV_VertexID,out float4 pos:SV_Position,out float2 uv:TEXCOORD0){",
    "uv=float2((vid<<1)&2,vid&2);pos=float4(uv.x*2-1,1-uv.y*2,0,1);}",
    "float4 PSMain(float4 pos:SV_Position,float2 uv:TEXCOORD0):SV_Target{",
    "float3 c=max(0.0,gTex.Load(int3(int2(pos.xy),0)).rgb);",
    "float3 st=step(0.0031308,c);",
    "float3 e=lerp(c*12.92,1.055*pow(abs(c),1.0/2.4)-0.055,st);",
    "float t=frac(gT*6.2272);float2 p=pos.xy;",
    "float ax=frac(p.x*211.1488),ay=frac(p.y*210.944);",
    "float d1=dot(float3(ax,ay,t),float3(ay+33.33,ax+33.33,t+33.33));",
    "float u1=d1+ax,v1=d1+ay,w1=u1+v1;",
    "float3 n1=float3(frac(w1*(d1+t)),frac((u1*2)*v1),frac(w1*u1));",
    "float2 q=p+64.0;",
    "float bx=frac(q.x*211.1488),by=frac(q.y*210.944);",
    "float d2=dot(float3(bx,by,t),float3(by+33.33,bx+33.33,t+33.33));",
    "float u2=d2+bx,v2=d2+by,w2=u2+v2;",
    "float3 n2=float3(frac(w2*(d2+t)),frac((u2*2)*v2),frac(w2*u2));",
    "float3 vv=e*510.0;float3 edge=min(min(float3(1,1,1),vv),510.0-vv);",
    "float3 o=((n1-0.5)+edge*(n2-0.5))*(1.0/255.0)+e;",
    "return float4(o,1);}"
);

#[cfg(test)]
mod shader_parity;
