// Replacement pixel shader for the holographic sight's reticle quad.
//
// Identified live as PS hash 66394C5F4B95AB9A (6271 bytes, DXIL / shader model 6.0), drawn as a
// 6-index instanced quad inside CRenderNode_RenderElements. Confirmed by removal: dropping this
// draw removes the reticle and nothing else. Game 2.5: EE25C03C35C017BD (6279 bytes), the same
// code with wider constant buffer declarations -- the rows read here did not move.
//
// WHAT THE ORIGINAL DOES, and why it is wrong in VR
// -------------------------------------------------
// It samples the reticle atlas four times: once at the quad's own uv, and three more times
// displaced along the view direction in tangent space. Measured material constants:
//
//     _29_m0[7].x  = 0.1                     displacement scale
//     _29_m0[8]    = (1, 0.1, 0.075, 0.05)   weights of the four layers
//     _29_m0[12]   = (0.5, 0, 1, 0.5)        atlas rect of the reticle
//
// The crisp reticle is the FIRST layer -- the one with zero displacement. It is painted on the
// glass and does not move with the eye at all; the displaced layers are a faint shimmer. On a
// flat screen the eye is always on the sight's axis, so nothing gives it away. In VR you can
// look at the glass from the side and the dot follows you, which a collimated sight never does.
//
// WHAT THIS DOES INSTEAD
// ----------------------
// A collimated sight projects its reticle to infinity along the bore. The eye therefore sees the
// dot at the one point on the glass where the line of sight is parallel to the bore, and sees
// nothing once that point leaves the window. That is an angular lookup, not a positional one:
//
//     uv = 0.5 + dist * float2(dot(V,T), dot(V,B)) / glassSizePerUv
//
// with V the direction from eye to pixel and dist their distance. The scale is DERIVED, not
// tuned: glassSizePerUv comes from the screen-space derivatives of the interpolated world
// position, so the reticle keeps exactly the apparent size the artist authored and there is no
// magic number to fit. The same construction is what the Crysis VR mod does with an explicit
// world-space aim point; here the aim point is at infinity, which is what a collimator gives.
//
// The dot vanishing off-axis needs no extra code: the sample coordinate is clamped into the
// atlas rect with a 1% inset (the original's own clamp, kept below), and the reticle's border is
// empty. That is the same mechanism the Crysis shader relies on.
//
// Signature reproduced from the original container's ISG1 chunk -- SV_Position0 plus TEXCOORD0..4
// at registers 0..5. The decompiler renamed those to TEXCOORD1..5 and dropped SV_Position; using
// its names would produce a signature the vertex shader does not match and the PSO would fail.

// ---- the two tunables, both physical ---------------------------------------------------------
// SIGHT_SIZE_SCALE corrects the glass size the vertex shader measures, if it turns out to be off
// by a constant. It is a CHECK, not a taste setting: the reticle's apparent size is a direct
// readout of it. At the correct value the collimated reticle is exactly as large as the stock
// one, because the mapping is then a pure translation; if it looks N times smaller, this is N.
#ifndef SIGHT_SIZE_SCALE
#define SIGHT_SIZE_SCALE 1.0f
#endif
// SIGHT_RETICLE_DISTANCE is where the reticle is projected, in metres. A real collimator puts it
// at infinity, which is the default here. Finite values are not a fudge: the Crysis VR mod uses
// 10 m deliberately, because a reticle at infinity tracks head movement one-for-one and in VR the
// eye is not held steady by a cheek weld, so tracking noise shows up as a wandering dot.
// The blend is exact -- k = D/(d+D) with d the eye-to-glass distance -- so 0 reproduces the stock
// painted-on reticle and a large value reproduces full collimation.
#ifndef SIGHT_RETICLE_DISTANCE
#define SIGHT_RETICLE_DISTANCE 1.0e6f
#endif
// ZEROING. The collimation above is built around the glass NORMAL -- the dot sits where the line
// of sight through the glass is perpendicular to it. A bullet does not care about the glass: it
// leaves along the bore, and a reflex sight's window is deliberately tilted relative to that. So
// unless the model happens to mount the glass square to the bore, the dot is off by that tilt.
//
// The bore direction is known to the mod (the plugin publishes the muzzle forward in shared slots
// [24..26], which is what the barrel dot draws from) but NOT reachable from here: the root
// signature is the game's and there is no spare constant to carry it in.
//
// So the sight is zeroed the way a real one is -- by moving the reticle on the glass. Units are
// uv, i.e. 1.0 is the full width of the reticle across the window. The dot moves OPPOSITE to the
// sign, exactly like a windage screw.
#ifndef SIGHT_USE_BORE
#define SIGHT_USE_BORE 1
#endif
// Draws a bright mark at the QUAD'S OWN CENTRE -- the place the stock shader paints the reticle,
// i.e. the geometric centre of the glass. The collimated reticle sits at the eye's lateral offset
// from the sight axis, so the gap between the two marks, measured in window widths, IS that
// offset divided by the glass width. One look per eye and the number stops being a deduction:
// if the eyes really are 65 mm apart on a 26 mm window, one of them must show the gap at 2.5
// widths -- and if instead both gaps are small, the glass is wider than the vertex buffer said
// and sizeU is the thing that is wrong.
#ifndef SIGHT_DEBUG_CENTRE
#define SIGHT_DEBUG_CENTRE 0
#endif

// RETICLE ANGULAR SIZE. A collimator projects its reticle to infinity, so the reticle subtends
// the same angle wherever the eye is: moving the head closer to the window shows more window
// around it, never a bigger ring. Mapping the glass 1:1 onto the atlas (uv offset = world
// offset / glass size) fixes the reticle's size ON THE GLASS instead, so its angle grew as 1/d --
// at 5 cm a ring filled the window and was cut by its edges off-axis. The uv offset is therefore
// the line of sight's ANGLE off the bore (its tangent) times this distance: the reticle has its
// authored glass size, and today's look, for an eye this far from the glass, and the same angle
// at every other distance. 15 cm is a cheek weld behind a rifle's holographic sight.
#ifndef SIGHT_RETICLE_RELIEF
#define SIGHT_RETICLE_RELIEF 0.15f
#endif

// SCOPE BUILD. A magnified scope's reticle is authored to fill its eyepiece, not to subtend a
// collimator's angle: at the angular size it shrank to a third at 5 cm and short_06's red filter
// covered only a band of the window. Compiled once more with SIGHT_SCOPE_GLASS=1 into
// CyberpunkVR_SightPsScope.dxil; the plugin binds that twin while the optics path is 'shader'.
// The reticle then keeps its glass size (uv offset = world offset / glass size), still centred on
// the line of sight along the bore.
#ifndef SIGHT_SCOPE_GLASS
#define SIGHT_SCOPE_GLASS 0
#endif
// SCOPE LABELS, A LITTLE BRIGHTER. The shader scopes' panels and Rasetsu's LCD labels are drawn at
// this multiple of their native emission; against a bright daytime image the native level read as
// washed out. The camera scopes' panels have their own emission (nativeOpticMarks).
#ifndef SIGHT_LABEL_GAIN
#define SIGHT_LABEL_GAIN 1.5f
#endif
// COLLIMATOR MARKS OVER A BRIGHT BACKGROUND. A reflex sight's reticle is native transparent UI added
// to the scene, and the dim ones -- short_01's frame and brackets (EmissiveEV 5), short_03's ring (7)
// -- vanished against the sky. A mark is lifted SIGHT_COLLIMATOR_GAIN times, up to
// SIGHT_COLLIMATOR_LIFT (brighter ones keep their level: short_04/05's dots, EmissiveEV 25-80), and
// covers the background behind it from SIGHT_COLLIMATOR_OPAQUE on, instead of only adding to it.
#ifndef SIGHT_COLLIMATOR_LIFT
#define SIGHT_COLLIMATOR_LIFT 2.5f
#endif
#ifndef SIGHT_COLLIMATOR_GAIN
#define SIGHT_COLLIMATOR_GAIN 2.5f
#endif
#ifndef SIGHT_COLLIMATOR_OPAQUE
#define SIGHT_COLLIMATOR_OPAQUE 0.6f
#endif
#ifndef SIGHT_COLLIMATOR_COVER
#define SIGHT_COLLIMATOR_COVER 0.85f
#endif

// FIXED FRAME, COLLIMATED DOT. The whole atlas rect used to be collimated: the reticle AND the
// frame/ring around it slid across the glass with the line of sight. Only the dot belongs to the
// bore; the frame is painted on the glass. With SIGHT_FRAME_FIXED=1 the atlas is sampled twice: at the
// collimated coordinate, kept only inside SIGHT_DOT_RADIUS of the rect centre (the dot), and at the
// stock painted-on coordinate, kept only outside that radius (the frame). The radius is in atlas-rect
// units (0.5 = the whole rect); the edge is soft over SIGHT_DOT_SOFT of it.
#ifndef SIGHT_FRAME_FIXED
#define SIGHT_FRAME_FIXED 1
#endif
#ifndef SIGHT_DOT_RADIUS
#define SIGHT_DOT_RADIUS 0.12f
#endif
#ifndef SIGHT_DOT_SOFT
#define SIGHT_DOT_SOFT 0.25f
#endif
// Faint background inside the dot region (Add-Vantage's 4 % box and glows) is not part of the dot: the
// collimator lift turned it into a visible square. Whatever is below this level in the collimated
// sample is dropped, and the rest is rescaled so the dot keeps its full level.
#ifndef SIGHT_DOT_FLOOR
#define SIGHT_DOT_FLOOR 0.15f
#endif

#ifndef SIGHT_ZERO_U
#define SIGHT_ZERO_U 0.0f
#endif
#ifndef SIGHT_ZERO_V
#define SIGHT_ZERO_V 0.0f
#endif

cbuffer GlobalShaderConstants : register(b0, space0) { float4 _19_m0[30]; };
cbuffer CameraShaderConstants : register(b1, space0) { float4 _24_m0[53]; };
cbuffer ShaderSpecificConsts  : register(b4, space0) { float4 _29_m0[21]; };

Texture2D<float4> _11[32768] : register(t0, space1);   // bindless: the noise map
Texture2D<float4> _13        : register(t0, space0);   // the reticle atlas
SamplerState      _32        : register(s0, space0);

static const float _47[4] = { 1.0f, 0.5f, 0.0f, 0.0f };
static const float _50[4] = { 1.0f, 0.0f, 1.0f, 0.0f };
static const float _52[4] = { 1.0f, 1.0f, 0.0f, 1.0f };

struct PSIn {
    float4 pos      : SV_Position;   // reg 0, unused -- declared so the signature matches
    float4 TEXCOORD : TEXCOORD0;     // reg 1: uv.xy, view z, 0
    float4 COLOR    : TEXCOORD1;     // reg 2: vertex colour (unused by the original)
    float4 NRM_BX   : TEXCOORD2;     // reg 3: normal.xyz, bitangent.x
    float4 BYZ_TXY  : TEXCOORD3;     // reg 4: bitangent.yz, tangent.xy
    float4 TZ_WPOS  : TEXCOORD4;     // reg 5: tangent.z, precise eye-to-pixel xyz (paired VS)
};

// PANELS OVER THE WHOLE GLASS. A native scope panel projected from where it hangs behind the lens
// is smaller than the glass: its backdrop showed as a rectangle inside the lens, sliding with the
// eye (long_02, long_05, Osprey). Laid on the lens by the lens's own uv instead (Osprey, 14.10) it
// filled the glass but stood still on it while the image is sampled by the viewing ray, so the
// crosshair left the aim point as soon as the eye was off centre, and the shot missed it.
// Filled, a panel keeps the projected panel's centre -- where the eye's ray along the bore meets
// the glass, the aim point -- and is sized to cover the glass (its physical size from the vertex
// shader) at the panel's own aspect, a fifth larger so an eye a little off centre does not bring
// its edge inside; the edge itself is soft.
#ifndef SIGHT_PANEL_FILL_MARGIN
#define SIGHT_PANEL_FILL_MARGIN 1.2f
#endif
float2 panelFillExtent(float2 panel,float2 glass) {
    const float2 p=max(panel,1e-5f);
    return p*max(glass.x/p.x,glass.y/p.y)*SIGHT_PANEL_FILL_MARGIN;
}
float panelEdgeFade(float2 localUV) {
    const float2 e=min(localUV,1.0f-localUV);
    return saturate(min(e.x,e.y)/0.03f);
}

// CAMERA SCOPE MARKS, READABLE OVER THE IMAGE. The native panels are dim translucent UI: glyph
// levels of 0.04-1.0 in the atlas, most of them over a backdrop (Nekomata's green disc, sniper_03's
// blue gradient), and their native emission (EmissiveEV 1-30) leaves labels and reticles faint over
// a daylight image -- at a native level the glyphs were invisible on the sky. So each panel's ink is
// split into its local backdrop (the darkest of 8 samples on a ring of SIGHT_MARK_RING tile widths:
// fills, gradients and discs are wider than that, strokes and text narrower) and the glyphs above it
// (above the backdrop by SIGHT_MARK_FLOOR of itself: a backdrop's own noise texture is no glyph --
// sniper_03's turned into blue grain over the sky). The glyphs get the panel's emission -- authored
// per panel so that its glyphs reach one common level (tools/optics_tests/tune_scope_panels.py),
// at most SIGHT_MARK_PEAK -- and cover the image behind them from SIGHT_MARK_OPAQUE on, with a dark
// rim one pixel round them; the backdrop stays a faint additive tint at SIGHT_MARK_BACKDROP whatever
// the panel's emission. Strokes are taken at the brightest sample over the pixel's footprint: one
// screen pixel spans several atlas texels here, and a hair a texel wide would otherwise fall between
// samples.
#ifndef SIGHT_MARK_RING
#define SIGHT_MARK_RING 0.03f
#endif
#ifndef SIGHT_MARK_FLOOR
#define SIGHT_MARK_FLOOR 0.6f
#endif
#ifndef SIGHT_MARK_BACKDROP
#define SIGHT_MARK_BACKDROP 2.5f
#endif
#ifndef SIGHT_MARK_PEAK
#define SIGHT_MARK_PEAK 4.0f
#endif
#ifndef SIGHT_MARK_OPAQUE
#define SIGHT_MARK_OPAQUE 1.0f
#endif
#ifndef SIGHT_MARK_COVER
#define SIGHT_MARK_COVER 0.9f
#endif
#ifndef SIGHT_MARK_RIM
#define SIGHT_MARK_RIM 0.6f
#endif
// GLASS TINTED BY ITS PANEL. A panel whose backdrop is the colour of the glass itself -- Grad's
// (sniper_01) canvas is dark green edge to edge, its labels red over it -- tints the image by that
// colour: the image is filtered by the backdrop's hue (its brightest channel kept) at the panel's
// tint strength, authored per panel (front EmissiveEVRaytracingBias, rear EmissiveDirectionality,
// both 0 by default and unused by this surface otherwise). The additive backdrop at
// SIGHT_MARK_BACKDROP is far too dark to show it: 0.0145 at its brightest. Full strength from a
// backdrop of SIGHT_MARK_TINT_LEVEL on; a black canvas tints nothing.
#ifndef SIGHT_MARK_TINT_LEVEL
#define SIGHT_MARK_TINT_LEVEL 0.005f
#endif

float maxOf(float3 c) { return max(c.x, max(c.y, c.z)); }

// A panel's canvas sits in its tile as the ink lays it out: scaled to the tile's width (or height,
// for a tall canvas) and against its top left corner. aspect = canvas width / height, 0 = square.
// sniper_03's 1481x960 canvas fills the top 65 % of its tile, long_05's 1400x530 the top 38 %.
float2 canvasExtent(float aspect) {
    return aspect >= 1.0f ? float2(1.0f, 1.0f / aspect) : aspect > 0.0f ? float2(aspect, 1.0f) : 1.0f;
}

// fill > 0: the panel covers the glass, this many metres across, centred on the aim point
// (panelFillExtent). size: projected panels are drawn at this multiple of their size.
// projection.w is the panel's emission (see above). filter: what the image is multiplied by
// before the marks go over it (tint, SIGHT_MARK_TINT_LEVEL).
float4 nativeOpticMarks(float2 slope,float axial,float4 projection,float2 tile,float aspect,float size,float2 fill,
                        float tint,out float3 filter) {
    filter=1.0f;
    if(projection.w<=0.0f)return 0.0f;
    const float2 localUV=fill.x>0.0f ? 0.5f+slope*axial/fill :
        0.5f+slope*max(axial+projection.z,1e-5f)/max(projection.xy*size,1e-5f);
    if(any(localUV<0.0f)||any(localUV>1.0f))return 0.0f;
    const float2 canvas=canvasExtent(aspect);
    const float2 lo=tile*.5f, hi=(tile+canvas)*.5f;
    const float2 uv=lo+localUV*canvas*.5f;
    uint w,h;_13.GetDimensions(w,h);
    const float2 texel=1.0f/float2(w,h);
    const float2 px=max(fwidth(uv),texel);
    const float2 inLo=lo+texel*.5f, inHi=hi-texel*.5f;
    float3 ink=0.0f;
    [unroll] for(int x=-1;x<=1;++x)
        [unroll] for(int y=-1;y<=1;++y)
            ink=max(ink,_13.SampleLevel(_32,clamp(uv+float2(x,y)*px*.45f,inLo,inHi),0).rgb);
    ink=max(ink,0.0f);
    const float ring=SIGHT_MARK_RING*.5f;
    float3 backdrop=ink;
    [unroll] for(int i=0;i<8;++i) {
        float s,c;sincos(i*0.7853981634f,s,c);
        backdrop=min(backdrop,max(_13.SampleLevel(_32,clamp(uv+float2(c,s)*ring,inLo,inHi),0).rgb,0.0f));
    }
    const float3 floorLevel=backdrop*(1.0f+SIGHT_MARK_FLOOR);
    float3 glyph=max(ink-floorLevel,0.0f)*projection.w;
    glyph*=min(1.0f,SIGHT_MARK_PEAK/max(maxOf(glyph),1e-6f));
    float rim=0.0f;
    [unroll] for(int j=0;j<4;++j) {
        const float2 o=float2(j==0?-px.x:j==1?px.x:0.0f, j==2?-px.y:j==3?px.y:0.0f)*1.5f;
        rim=max(rim,maxOf(max(_13.SampleLevel(_32,clamp(uv+o,inLo,inHi),0).rgb-floorLevel,0.0f)));
    }
    const float fade=fill.x>0.0f ? panelEdgeFade(localUV) : 1.0f;
    if(tint>0.0f) {
        const float level=maxOf(backdrop);
        filter=lerp(1.0f,backdrop/max(level,1e-6f),saturate(tint)*saturate(level/SIGHT_MARK_TINT_LEVEL)*fade);
    }
    const float3 color=(backdrop*SIGHT_MARK_BACKDROP+glyph)*fade;
    const float cover=saturate(maxOf(glyph)/SIGHT_MARK_OPAQUE)*SIGHT_MARK_COVER;
    const float edge=saturate(rim*projection.w/SIGHT_MARK_OPAQUE)*SIGHT_MARK_RIM;
    return float4(color,max(cover,edge)*fade);
}

// Only the port's built-in sight surfaces use these materials. Size the ink in
// view angles so leaning along the glass does not resize or duplicate the mark.
float4 integratedReticle(PSIn input, float style) {
    const float3 t = normalize(float3(input.BYZ_TXY.zw, input.TZ_WPOS.x));
    const float3 b = normalize(float3(input.NRM_BX.w, input.BYZ_TXY.xy));
    const float3 ray = input.TZ_WPOS.yzw;
    const float axial = dot(ray, normalize(input.NRM_BX.xyz));
    const float2 slope = (float2(dot(ray,t),dot(ray,b)) - axial * input.COLOR.zw) / max(abs(axial), 1e-5f);
    const float2 a = abs(slope);
    const float radius = length(slope);
    const float px = max(max(fwidth(slope.x),fwidth(slope.y)), 1e-5f);
    const float dotEdge = radius - (style == 1.0f ? .0016f : .0021f);
    // Four open arcs, with a quiet centre. The smart sights use short corner
    // brackets to distinguish the mark from the game's separate lock-on HUD.
    float frame = max(abs(radius - .013f) - .0011f, .0035f - min(a.x,a.y));
    if (style == 2.0f)
        frame = max(abs(max(a.x,a.y) - .012f) - .0011f, .006f - min(a.x,a.y));
    // Senkoh LX has four rounded ivory corners around an open centre.
    // Keep them in angular space so the native atlas' dot mask cannot cut them in half.
    if (style == 3.0f) {
        const float2 q = a - .009f;
        const float rounded = length(max(q,0.0f)) + min(max(q.x,q.y),0.0f) - .003f;
        frame = max(abs(rounded) - .0013f, .003f - min(a.x,a.y));
    }
    const float edge = min(dotEdge,frame);
    const float ink = saturate(.5f - edge / px);
    const float coverage = saturate(.5f - (edge - .8f * px) / px);
    const float3 color = style == 3.0f ? float3(1.26f,1.13f,.75f) :
                         style == 0.0f ? float3(1.4f,.72f,.27f) : float3(.46f,1.22f,1.03f);
    return float4(color * ink, coverage);
}

float4 main(PSIn IN) : SV_Target {
    if (_29_m0[6].x == -718.25f)
        return integratedReticle(IN, _29_m0[8].x);
#if SIGHT_SCOPE_GLASS
    // MAGNIFIED SCOPE PANELS (optics path 'shader'). A scope's native rear panel -- the
    // crosshair of long_01..03 -- hangs behind the lens, and the glass drawn over it hid
    // it. The port's reticle surfaces (ImageScale -715.25) sit 1-2 mm in front of the
    // glass and carry each native panel's tile (TexturePartUV), seen where the native
    // panel hangs: IntensityPerLayer xy = its size, z = its depth behind the surface,
    // w = 1 for the rear panel; EmissiveEV is the native panel's. LayersSeparation 2
    // lays the panel over the whole glass instead, centred on the aim point
    // (panelFillExtent: long_02, long_05, whose backdrops showed as rectangles). The
    // front panel adds light like the native transparent UI; the rear reticle keeps a
    // dark outline so a thin crosshair reads over a bright image.
    if (_29_m0[6].x == -715.25f) {
        const float3 lensT = normalize(float3(IN.BYZ_TXY.z, IN.BYZ_TXY.w, IN.TZ_WPOS.x));
        const float3 lensB = normalize(float3(IN.NRM_BX.w, IN.BYZ_TXY.x, IN.BYZ_TXY.y));
        const float3 lensN = normalize(IN.NRM_BX.xyz);
        const float3 eyeRay = IN.TZ_WPOS.yzw;
        const float axial = abs(dot(eyeRay, lensN));
        if (axial < 1e-5f) discard;
        const float4 panel = _29_m0[8];
        const float2 slope = float2(dot(eyeRay,lensT), dot(eyeRay,lensB)) / axial;
        const bool fill = (uint(round(_29_m0[7].x)) & 2u) != 0u;
        const float2 localUV = fill ? 0.5f + slope * axial / panelFillExtent(panel.xy, IN.COLOR.xy)
                                    : 0.5f + slope * max(axial + panel.z, 1e-5f) / max(panel.xy, 1e-5f);
        if (any(localUV < 0.0f) || any(localUV > 1.0f)) discard;
        const float fade = fill ? panelEdgeFade(localUV) : 1.0f;
        float2 uv = lerp(_29_m0[12].xy, _29_m0[12].zw, localUV);
        if (_29_m0[15].x != 0.0f) uv.y = 1.0f - uv.y;
        const bool rear = panel.w > 0.5f;
        float3 ink = max(_13.SampleLevel(_32, uv, 0).rgb, 0.0f);
        float outline = 0.0f;
        if (rear) {
            // A crosshair hair is a texel or two wide while one screen pixel spans several
            // atlas texels here: the ink is the brightest sample over the pixel's whole
            // footprint, so thin hairs never fall between samples.
            uint w, h; _13.GetDimensions(w, h);
            const float2 px = max(fwidth(uv), 1.0f / float2(w, h));
            [unroll] for (int x = -2; x <= 2; ++x)
                [unroll] for (int y = -2; y <= 2; ++y)
                    ink = max(ink, _13.SampleLevel(_32, uv + float2(x, y) * px * .25f, 0).rgb);
            const float3 border = max(max(_13.SampleLevel(_32, uv - float2(px.x, 0), 0).rgb,
                                          _13.SampleLevel(_32, uv + float2(px.x, 0), 0).rgb),
                                      max(_13.SampleLevel(_32, uv - float2(0, px.y), 0).rgb,
                                          _13.SampleLevel(_32, uv + float2(0, px.y), 0).rgb));
            outline = saturate(max(border.x, max(border.y, border.z)) * _29_m0[2].x) * .8f * fade;
        }
        const float3 color = ink * max(_29_m0[2].x, 0.0f) * SIGHT_LABEL_GAIN * fade;
        const float coverage = saturate(max(color.x, max(color.y, color.z)));
        // Premultiplied over (ONE, INV_SRC_ALPHA): alpha 0 adds, alpha 1 replaces.
        return float4(color, rear ? max(outline, coverage) : 0.0f);
    }
#else
    // Native UI controllers still render both reticle panels into the weapon's
    // shared atlas. Their old meshes must not blend a second time: the camera
    // surface below draws image + front marks + central reticle in one pass.
    if (_29_m0[6].x == -715.25f) discard;
#endif
    // Dedicated camera-image surface. ImageScale is a reserved exact marker
    // in the authored VR lens material; all native reticle materials continue
    // through the existing collimator path. A full scene image must use the
    // lens UVs, never the eye-dependent reticle offset below.
    const bool integratedCamera = _29_m0[6].x == -717.25f;
    if (_29_m0[6].x == -713.25f || _29_m0[6].x == -714.25f || _29_m0[6].x == -716.25f || integratedCamera) {
        const bool composite = _29_m0[6].x == -714.25f;
        const bool marks = _29_m0[6].x == -716.25f;
        const float3 lensT = normalize(float3(IN.BYZ_TXY.z, IN.BYZ_TXY.w, IN.TZ_WPOS.x));
        const float3 lensB = normalize(float3(IN.NRM_BX.w, IN.BYZ_TXY.x, IN.BYZ_TXY.y));
        const float3 lensN = normalize(IN.NRM_BX.xyz);
        const float3 eyeRay = IN.TZ_WPOS.yzw;
        const float axial = abs(dot(eyeRay, lensN));
        // Camera FOV is 2*atan(tan(40deg)/nativeMagnification). Sampling
        // by the viewing ray gives tan(apparentAngle)=M*tan(worldAngle)
        // at every eye-to-lens distance. UVs alone would behave as a monitor.
        // An 80-degree image circle also covers the widest native aperture
        // at 5 cm eye relief. Extra field coverage does not change magnification.
        const float imageHalfSlope = 0.8390996312f; // tan(80deg/2), optics/policy.lua
        const float2 localUV = 0.5f + float2(dot(eyeRay,lensT),dot(eyeRay,lensB)) /
            (2.0f * imageHalfSlope * max(axial,1e-5f));
        const bool rayVisible = axial >= 1e-5f && all(localUV >= 0.0f) && all(localUV <= 1.0f);
        if (!rayVisible && !composite)
            return float4(0.0f,0.0f,0.0f,1.0f);
        const float2 safeUV = saturate(localUV);
        const float2 cameraUV = composite ? float2(safeUV.x * 0.5f, safeUV.y) : safeUV;
        float2 uv = lerp(_29_m0[12].xy, _29_m0[12].zw, cameraUV);
        if (_29_m0[15].x != 0.0f) uv.y = 1.0f - uv.y;
        // The camera has already rendered and exposed the world. Ink stores
        // this image at half intensity, just like the native HUD. Undo that
        // storage factor; a monitor's EmissiveEV would expose the camera again.
        float3 scene = max(_13.SampleLevel(_32, uv,0).rgb, 0.0f) * 2.0f;
        if (!rayVisible) scene = 0.0f;
        if (integratedCamera) {
            const float4 reticle = integratedReticle(IN, _29_m0[8].x);
            return float4(scene * (1.0f - reticle.a) + reticle.rgb, 1.0f);
        }
        if (composite) {
            // Camera and the native LCD labels occupy separate atlas halves.
            // The labels stay on the physical screen as eye distance changes.
            // The whole widget spans the screen; its label sizes are set in the
            // widget itself (rasetsu_view.inkwidget).
            float2 labelsUV = float2(0.5f + saturate(IN.TEXCOORD.x) * 0.5f,
                                     1.0f - saturate(IN.TEXCOORD.y));
            labelsUV = lerp(_29_m0[12].xy, _29_m0[12].zw, labelsUV);
            if (_29_m0[15].x != 0.0f) labelsUV.y = 1.0f - labelsUV.y;
            const float3 labels = max(_13.Sample(_32, labelsUV).rgb, 0.0f) * _29_m0[8].y * SIGHT_LABEL_GAIN;
            // Native transparent-UI materials derive coverage from RGB with
            // AlphaAsOne=0; the UI atlas alpha is not reliable for empty pixels.
            const float coverage = saturate(max(labels.x, max(labels.y, labels.z)));
            scene = scene * (1.0f - coverage) + labels;
            // Aim mark. The camera looks along the bore from the weapon front, so the
            // bore is the image centre: the ray along the screen normal. Drawn in the
            // image's own angular units, it stays on the target at any eye distance.
            if (rayVisible) {
                const float2 sl = float2(dot(eyeRay,lensT), dot(eyeRay,lensB)) / max(axial,1e-5f);
                const float px = max(max(fwidth(sl.x), fwidth(sl.y)), 1e-6f);
                const float2 a = abs(sl);
                const float halfW = max(0.0012f, px * 0.7f);
                const float gap = 0.008f, arm = 0.050f, dotR = max(0.0016f, px);
                float dLine = 1e9f;
                if (a.x >= gap && a.x <= arm) dLine = min(dLine, a.y);
                if (a.y >= gap && a.y <= arm) dLine = min(dLine, a.x);
                const float dDot = length(sl);
                const float ink = max(saturate((halfW - dLine) / px + 0.5f), saturate((dotR - dDot) / px + 0.5f));
                const float edge = 1.5f * px;
                const float halo = max(saturate((halfW + edge - dLine) / px + 0.5f),
                                       saturate((dotR + edge - dDot) / px + 0.5f));
                scene *= 1.0f - 0.8f * halo;
                scene = lerp(scene, float3(0.80f, 1.0f, 0.95f) * 3.0f, ink);
            }
        }
        if(marks) {
            const float2 slope=float2(dot(eyeRay,lensT),dot(eyeRay,lensB))/max(axial,1e-5f);
            // A panel with a backdrop is stretched over the glass: projected from where it hangs,
            // its backdrop slid across the image as a rectangle with the eye (Osprey's whole HUD,
            // Nekomata's disc 3.4 cm inside the scope, Grad's and sniper_03's full-canvas HUDs).
            // Native ADS shows both panels of a sniper scope at screen size, so both are filled.
            // A filled panel needs no depth, so its projection's z carries -(canvas aspect)
            // instead (canvasExtent; at most -1, while a projected panel's depth is a few
            // centimetres either way).
            // On 2.5 this draw receives LayersSeparation, ScanlinesDensity, IsBroken,
            // ScanlinesIntensity and RenderTextureScale (registers 7, 9, 10, 11, 14) as zero
            // whatever the material says -- presumably the world-UI glitch parameters, which the
            // engine sets itself -- while every other register keeps the authored value
            // (measured 2026-10-26 with SIGHT_DEBUG_MARKS). The front projection stays in
            // IntensityPerLayer; the rear one, which lived in ScanlinesDensity and so was never
            // drawn, moved to FixToPbr, EdgeMask, FixForVerticalSlide and SaturationLift (13, 16,
            // 18, 20: x, y, z, w), which this branch does not otherwise read.
            const float4 frontP = _29_m0[8];
            const float4 rearP = float4(_29_m0[13].x, _29_m0[16].x, _29_m0[18].x, _29_m0[20].x);
            const bool frontFill = frontP.z <= -0.5f, rearFill = rearP.z <= -0.5f;
#if SIGHT_DEBUG_MARKS
            {
                // Diagnostics: 4 x 4 cells over the lens, row-major from the top left: register .x
                // of 2 3 4 5 / 7 9 10 11 / 13 14 16 17 / 18 19 20 and 9.w; green level = value / 20,
                // red = negative, blue = zero.
                const float2 q = saturate(0.5f + slope * 1.6f);
                const int cell = min(int(q.y * 4.0f), 3) * 4 + min(int(q.x * 4.0f), 3);
                static const int regs[16] = { 2, 3, 4, 5, 7, 9, 10, 11, 13, 14, 16, 17, 18, 19, 20, 9 };
                const float v = cell == 15 ? _29_m0[9].w : _29_m0[regs[cell]].x;
                const float2 f = frac(q * 4.0f);
                if (min(f.x, f.y) < 0.06f) return float4(0.0f, 0.0f, 0.0f, 1.0f);
                return float4(v < 0.0f ? 1.0f : 0.0f, v > 0.0f ? saturate(v / 20.0f) : 0.0f, v == 0.0f ? 1.0f : 0.0f, 1.0f);
            }
#endif
            float3 frontTint,rearTint;
            const float4 front=nativeOpticMarks(slope,axial,frontP,float2(1,0),frontFill ? -frontP.z : 1.0f,1.0f,
                frontFill ? panelFillExtent(frontP.xy, IN.COLOR.xy) : float2(0.0f,0.0f),_29_m0[3].x,frontTint);
            const float4 rear=nativeOpticMarks(slope,axial,rearP,float2(1,1),rearFill ? -rearP.z : 1.0f,1.0f,
                rearFill ? panelFillExtent(rearP.xy, IN.COLOR.xy) : float2(0.0f,0.0f),_29_m0[4].x,rearTint);
            // The glass tint first, then premultiplied: the glyphs cover the image, the backdrop adds to it.
            scene*=frontTint*rearTint;
            scene=scene*(1.0f-front.a)+front.rgb;
            scene=scene*(1.0f-rear.a)+rear.rgb;
        }
        return float4(scene, 1.0f);
    }
    const float3 T = float3(IN.BYZ_TXY.z, IN.BYZ_TXY.w, IN.TZ_WPOS.x);
    const float3 B = float3(IN.NRM_BX.w,  IN.BYZ_TXY.x, IN.BYZ_TXY.y);

    // Eye -> pixel, calculated by the paired VS before interpolating large world
    // coordinates. Each eye still uses its own camera position and projection.
    const float3 d3 = IN.TZ_WPOS.yzw;
    const float  dist = max(length(d3), 1e-6f);
    const float3 V = d3 / dist;

    const float vT = dot(V, T);
    const float vB = _29_m0[18].x * dot(V, B);

    // --- the atlas rect, exactly as the original ------------------------------------------
    const float _120 = 1.0f - IN.TEXCOORD.y;
    const float _121 = _29_m0[12].z - _29_m0[12].x;      // rect width
    const float _122 = _29_m0[12].w - _29_m0[12].y;      // rect height
    const float _126 = (_121 * 0.01f) + _29_m0[12].x;
    const float _127 = (_122 * 0.01f) + _29_m0[12].y;
    const float _131 = (_121 * 0.99f) + _29_m0[12].x;
    const float _132 = (_122 * 0.99f) + _29_m0[12].y;

    // --- collimation ------------------------------------------------------------------------
    // The glass size comes from the VERTEX shader now (see sight_reflex_vs.hlsl), in the slot
    // that used to carry the vertex colour. That removes ddx/ddy from the coordinate entirely:
    // the previous two attempts derived the scale from derivatives, and DLSS's per-frame
    // sub-pixel jitter moved the 2x2 derivative quads, which made the reticle's texels run back
    // and forth. This value is per-vertex and static.
    const float sizeU = IN.COLOR.x * SIGHT_SIZE_SCALE;
    const float sizeV = IN.COLOR.y * SIGHT_SIZE_SCALE;

    const float ucoord = IN.TEXCOORD.x;
    const float vcoord = _120;                 // the original's own v, 1 - uv.y
    // Collimate along the sight's OPTICAL AXIS, not the glass normal. The vertex shader hands
    // down that axis already resolved into this tangent frame (see sight_reflex_vs.hlsl); with
    // both terms zero this reduces exactly to the previous behaviour, so it is a generalisation
    // rather than a replacement.
    const float3 N = IN.NRM_BX.xyz;
    const float  dn = dot(d3, N);
#if SIGHT_USE_BORE
    const float2 bore = IN.COLOR.zw;
#else
    const float2 bore = float2(0.0f, 0.0f);
#endif
    // Tangent of the line of sight off the bore, scaled to the reticle's reference distance (see
    // SIGHT_RETICLE_RELIEF). dn is the same everywhere on a planar window, so the mapping stays
    // affine across the glass.
#if SIGHT_SCOPE_GLASS
    const float angular = 1.0f;
#else
    const float angular = SIGHT_RETICLE_RELIEF / max(abs(dn), 1e-4f);
#endif
    const float fT = (dot(d3, T) - dn * bore.x) * angular;
    const float fB = (dot(d3, B) - dn * bore.y) * _29_m0[18].x * angular;

    // fT is the eye-to-pixel vector on the glass's u axis: across the quad it is exactly
    // (pixel offset - eye offset) in world units, so dividing by the world size of one uv unit
    // puts the reticle centre where fT = 0 -- the point on the glass laterally aligned with the
    // eye, which is where a collimated sight puts its dot. Degenerate size: keep the stock
    // mapping rather than emit something arbitrary.
    // Reticle at a finite distance: k = D/(d+D). k = 1 is a collimator, k = 0 is paint on glass.
    const float kColl = SIGHT_RETICLE_DISTANCE / (dist + SIGHT_RETICLE_DISTANCE);

    float2 uvC;
    uvC.x = (sizeU > 1e-5f) ? (0.5f + fT / sizeU + SIGHT_ZERO_U) : ucoord;
    // Measured, not reasoned: with the minus sign the reticle moved the wrong way
    // vertically. The bitangent runs WITH the shader's own v (which is already 1 - uv.y).
    uvC.y = (sizeV > 1e-5f) ? (0.5f + fB / sizeV + SIGHT_ZERO_V) : vcoord;

    uvC = lerp(float2(ucoord, vcoord), uvC, kColl);

    const float _135 = (_121 * uvC.x) + _29_m0[12].x;
    const float _136 = (_122 * uvC.y) + _29_m0[12].y;

    float _146, _147, _148;
    if (_29_m0[15].x != 0.0f) { _146 = 1.0f - _136; _147 = 1.0f - _132; _148 = 1.0f - _127; }
    else                      { _146 = _136;        _147 = _127;        _148 = _132;        }

    const float _155 = min(max(_29_m0[7].x * _121, 0.0f), 0.5f);


    // Four layers as before. With the base coordinate now angular they read as a short smear
    // along the line of sight, which is what they were for; nothing here is invented.
    const float2 uvCrisp = float2(clamp(min(max(_135, _126), _131), 0.0f, 1.0f),
                                  clamp(min(max(_146, _147), _148), 0.0f, 1.0f));
    float4 _229 = _13.Sample(_32, uvCrisp);
#if !SIGHT_SCOPE_GLASS
    {
        // The crisp layer at the brightest sample over the pixel's footprint: a collimator's
        // hairlines are a texel or two in the atlas and fell between samples (SIGHT_COLLIMATOR_LIFT).
        uint tw, th; _13.GetDimensions(tw, th);
        const float2 fp = max(fwidth(uvCrisp), 1.0f / float2(tw, th)) * .45f;
        [unroll] for (int x = -1; x <= 1; ++x)
            [unroll] for (int y = -1; y <= 1; ++y)
                _229 = max(_229, _13.SampleLevel(_32, float2(clamp(min(max(uvCrisp.x + x * fp.x, _126), _131), 0.0f, 1.0f),
                                                             clamp(min(max(uvCrisp.y + y * fp.y, _147), _148), 0.0f, 1.0f)), 0));
    }
#endif
#if !SIGHT_SCOPE_GLASS && SIGHT_FRAME_FIXED
    // Dot = the collimated sample inside the radius; frame = the stock painted-on sample outside it.
    const float dotMaskC = 1.0f - smoothstep(SIGHT_DOT_RADIUS * (1.0f - SIGHT_DOT_SOFT), SIGHT_DOT_RADIUS,
                                             length(uvC - 0.5f));
    {
        const float2 uvS = float2(ucoord, vcoord);
        const float frameMask = smoothstep(SIGHT_DOT_RADIUS * (1.0f - SIGHT_DOT_SOFT), SIGHT_DOT_RADIUS,
                                           length(uvS - 0.5f));
        const float _sx = (_121 * uvS.x) + _29_m0[12].x;
        const float _sy = (_122 * uvS.y) + _29_m0[12].y;
        const float _sy2 = (_29_m0[15].x != 0.0f) ? 1.0f - _sy : _sy;
        const float2 uvStock = float2(clamp(min(max(_sx, _126), _131), 0.0f, 1.0f),
                                      clamp(min(max(_sy2, _147), _148), 0.0f, 1.0f));
        float4 stock = _13.Sample(_32, uvStock);
        uint sw, sh; _13.GetDimensions(sw, sh);
        const float2 sfp = max(fwidth(uvStock), 1.0f / float2(sw, sh)) * .45f;
        [unroll] for (int sx = -1; sx <= 1; ++sx)
            [unroll] for (int sy = -1; sy <= 1; ++sy)
                stock = max(stock, _13.SampleLevel(_32, float2(clamp(min(max(uvStock.x + sx * sfp.x, _126), _131), 0.0f, 1.0f),
                                                               clamp(min(max(uvStock.y + sy * sfp.y, _147), _148), 0.0f, 1.0f)), 0));
        _229 = max(max(_229 - SIGHT_DOT_FLOOR, 0.0f) / (1.0f - SIGHT_DOT_FLOOR) * dotMaskC, stock * frameMask);
    }
#endif
    float4 _235 = _13.Sample(_32, float2(clamp(min(max((_155 * vT) + _135, _126), _131), 0.0f, 1.0f),
                                         clamp(min(max((_155 * vB) + _146, _147), _148), 0.0f, 1.0f)));
    float4 _241 = _13.Sample(_32, float2(clamp(min(max(((vT * 2.0f) * _155) + _135, _126), _131), 0.0f, 1.0f),
                                         clamp(min(max(((vB * 2.0f) * _155) + _146, _147), _148), 0.0f, 1.0f)));
    float4 _247 = _13.Sample(_32, float2(clamp(min(max(((vT * 3.0f) * _155) + _135, _126), _131), 0.0f, 1.0f),
                                         clamp(min(max(((vB * 3.0f) * _155) + _146, _147), _148), 0.0f, 1.0f)));

#if !SIGHT_SCOPE_GLASS && SIGHT_FRAME_FIXED
    _235 = max(_235 - SIGHT_DOT_FLOOR, 0.0f) * dotMaskC; _241 = max(_241 - SIGHT_DOT_FLOOR, 0.0f) * dotMaskC; _247 = max(_247 - SIGHT_DOT_FLOOR, 0.0f) * dotMaskC;
#endif

    // --- everything below is the original, unchanged -----------------------------------------
    const float _259 = min(max(_29_m0[11].x, 0.75f), 1.0f);
    const float _262 = ((1.0f - _259) * _11[asuint(_29_m0[1]).x + 0u].Sample(_32,
        float2((_29_m0[9].z * _19_m0[0].x) + (_29_m0[9].x * IN.TEXCOORD.x),
               (_29_m0[9].w * _19_m0[0].x) + (_29_m0[9].y * IN.TEXCOORD.y))).x) + _259;

    const float _326 = 1.0f - ((((1.0f - ((_29_m0[8].y * _235.x) * _262)) * (1.0f - (_29_m0[8].x * _229.x))) * (1.0f - ((_262 * _241.x) * _29_m0[8].z))) * (1.0f - ((_262 * _247.x) * _29_m0[8].w)));
    const float _327 = 1.0f - ((((1.0f - ((_29_m0[8].y * _235.y) * _262)) * (1.0f - (_29_m0[8].x * _229.y))) * (1.0f - ((_262 * _241.y) * _29_m0[8].z))) * (1.0f - ((_262 * _247.y) * _29_m0[8].w)));
    const float _328 = 1.0f - ((((1.0f - ((_29_m0[8].y * _235.z) * _262)) * (1.0f - (_29_m0[8].x * _229.z))) * (1.0f - ((_262 * _241.z) * _29_m0[8].z))) * (1.0f - ((_262 * _247.z) * _29_m0[8].w)));
    const float _329 = 1.0f - ((((1.0f - ((_29_m0[8].y * _235.w) * _262)) * (1.0f - (_29_m0[8].x * _229.w))) * (1.0f - ((_262 * _241.w) * _29_m0[8].z))) * (1.0f - ((_262 * _247.w) * _29_m0[8].w)));

    const float _338 = (frac(_19_m0[0].x + 0.375f) * 6.0f) + 2.0f;
    const float _346 = frac(floor((_19_m0[0].x + 0.35747999f) * 60.0f) * 0.1031f);
    const float _349 = dot(_346.xxx, (_346 + 33.33f).xxx);
    const float _369 = frac(frac(((_346 + _346) + (_349 * 2.0f)) * (_349 + _346)) + IN.TEXCOORD.x)
                     / ((clamp(frac(((floor(_338 * IN.TEXCOORD.x) / _338) + 1.0f) * _19_m0[0].x), 0.0f, 1.0f) * 3.0f) + 1.0f);
    const float _378 = frac(((frac(_19_m0[0].x) * (floor(_369 * _338) / _338)) + (-0.16249999f)) * 0.1031f);
    const float _380 = dot(_378.xxx, (_378 + 33.33f).xxx);
    const uint  _391 = uint(int(frac(((_378 + _378) + (_380 * 2.0f)) * (_380 + _378)) * 4.0f));
    const float _400 = frac(_19_m0[0].x * 0.1031f);
    const float _402 = dot(_400.xxx, (_400 + 33.33f).xxx);
    const float _410 = frac(((_400 + _400) + (_402 * 2.0f)) * (_402 + _400));
    const float _417 = (_410 < 0.69999999f) ? 0.0f : 1.0f;
    const float _433 = frac(floor(_19_m0[0].x * 10.0f) * 0.1031f);
    precise float _435 = _433 * (_433 + 33.33f);
    precise float _436 = _435 * _435;

    float _445, _447, _449, _451;
    if (frac(_436 * 2.0f) < (1.0f - _29_m0[10].x)) {
        _445 = _326; _447 = _327; _449 = _328; _451 = _329;
    } else {
        if (((IN.TEXCOORD.x > 0.77999997f) && (IN.TEXCOORD.x < 0.85000002f))
         || (((IN.TEXCOORD.x > 0.019999999f) && (IN.TEXCOORD.x < 0.11999999f))
          || ((IN.TEXCOORD.x > 0.28000000f) && (IN.TEXCOORD.x < 0.54299998f)))) {
            float4 _544 = _13.Sample(_32, _369.xx);
            float4 _555 = _13.Sample(_32, float2(
                ((_417 * ((_410 * 0.024999998f) + (-0.019999999f))) + 0.0099999998f) + _135,
                ((_417 * ((_410 * 0.029999999f) + (-0.0099999998f))) + (-0.0099999998f)) + _146));
            _445 = (_555.x * 0.40000000f) + (_544.x * _47[_391]);
            _447 = (_555.y * 0.40000000f) + (_544.y * _50[_391]);
            _449 = (_555.z * 0.40000000f) + (_544.z * _52[_391]);
            _451 = (_555.w * 0.40000000f) + _544.w;
        } else {
            _445 = _326; _447 = _327; _449 = _328; _451 = _329;
        }
    }

    const float _461 = clamp((((IN.TEXCOORD.y * 4.0f) * _120) + 1.0f) - _29_m0[16].x, 0.0f, 1.0f);
    const float _473 = ((_461 * _445) * _29_m0[17].x) * _29_m0[2].x;
    const float _476 = ((_461 * _447) * _29_m0[17].y) * _29_m0[2].x;
    const float _479 = ((_461 * _449) * _29_m0[17].z) * _29_m0[2].x;
    const float _485 = clamp(max(max(min(1.0f, _473), min(1.0f, _476)), min(1.0f, _479)), 0.0f, 1.0f);
    const float _491 = (_29_m0[0].x * (_451 - _485)) + _485;
    const float _498 = ((1.0f - _491) * _29_m0[19].x) + _491;

    float4 o;
    o.x = min(max((-0.0f) - min((-0.0f) - (_473 * _498), 0.0f), 0.0f), 65000.0f);
    o.y = min(max((-0.0f) - min((-0.0f) - (_476 * _498), 0.0f), 0.0f), 65000.0f);
    o.z = min(max((-0.0f) - min((-0.0f) - (_479 * _498), 0.0f), 0.0f), 65000.0f);
    o.w = clamp(_498 * _29_m0[0].x, 0.0f, 1.0f);
#if SIGHT_SCOPE_GLASS
    o.xyz *= SIGHT_LABEL_GAIN;
#else
    {
        const float m = maxOf(o.xyz);
        if (m > 1e-5f) {
            const float lifted = max(m, min(m * SIGHT_COLLIMATOR_GAIN, SIGHT_COLLIMATOR_LIFT));
            o.xyz *= lifted / m;
            o.w = max(o.w, saturate(lifted / SIGHT_COLLIMATOR_OPAQUE) * SIGHT_COLLIMATOR_COVER);
        }
    }
#endif

#if SIGHT_DEBUG_CENTRE
    // Green cross-hair at the window's geometric centre, opaque and brighter than the reticle so
    // it cannot be mistaken for part of it.
    const float du_ = abs(ucoord - 0.5f);
    const float dv_ = abs(vcoord - 0.5f);
    if ((du_ < 0.006f && dv_ < 0.10f) || (dv_ < 0.006f && du_ < 0.10f)) {
        o = float4(0.0f, 8.0f, 0.0f, 1.0f);
    }
    // Ticks every 0.25 of a window width along the horizontal, so the gap can be READ rather than
    // estimated: one tick = a quarter of the glass width.
    const float g_ = abs(frac(ucoord * 4.0f + 0.5f) - 0.5f);
    if (g_ < 0.012f && dv_ > 0.34f && dv_ < 0.46f) {
        o = float4(0.0f, 3.0f, 4.0f, 1.0f);
    }
#endif
    return o;
}
