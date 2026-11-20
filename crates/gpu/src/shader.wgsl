// Triangles (ADR 0014), and the fills and copies of surfaces.

// --- Triangles ------------------------------------------------------------

struct DrawUniforms {
    // Combiners of colour and alpha: operation, first argument, second
    // argument. An argument is its source (0 diffuse, 1 current, 2 texture,
    // 3 specular), plus 16 to complement it, plus 32 to use its alpha in
    // every channel.
    color: vec4<u32>,
    alpha: vec4<u32>,
    // Alpha test (0 off, else 1 never .. 8 always), its reference value
    // (0 to 255), whether specular is added, whether a colour key applies.
    test: vec4<u32>,
    // The colour of the texels that are not drawn.
    key: vec4<f32>,
    // The fog colour; the fourth component is not zero when there is fog.
    fog: vec4<f32>,
}

@group(0) @binding(0) var<uniform> draw: DrawUniforms;
@group(1) @binding(0) var texture: texture_2d<f32>;
@group(1) @binding(1) var texture_sampler: sampler;

struct VertexInput {
    @location(0) position: vec4<f32>,
    // Colours arrive as the bytes of 0xAARRGGBB in memory order: blue first.
    @location(1) diffuse: vec4<f32>,
    @location(2) specular: vec4<f32>,
    @location(3) uv: vec2<f32>,
}

struct Smooth {
    @builtin(position) position: vec4<f32>,
    @location(0) diffuse: vec4<f32>,
    @location(1) specular: vec4<f32>,
    @location(2) uv: vec2<f32>,
}

@vertex
fn vertex_smooth(input: VertexInput) -> Smooth {
    return Smooth(input.position, input.diffuse.zyxw, input.specular.zyxw, input.uv);
}

fn argument(selector: u32, diffuse: vec4<f32>, texel: vec4<f32>, specular: vec4<f32>) -> vec4<f32> {
    var value: vec4<f32>;
    switch selector & 15u {
        case 2u { value = texel; }
        case 3u { value = specular; }
        // With one stage, "current" is the diffuse colour.
        default { value = diffuse; }
    }
    if (selector & 16u) != 0u {
        value = vec4<f32>(1.0) - value;
    }
    if (selector & 32u) != 0u {
        value = value.wwww;
    }
    return value;
}

fn combine(settings: vec4<u32>, diffuse: vec4<f32>, texel: vec4<f32>, specular: vec4<f32>) -> vec4<f32> {
    let first = argument(settings.y, diffuse, texel, specular);
    let second = argument(settings.z, diffuse, texel, specular);
    var result: vec4<f32>;
    switch settings.x {
        case 0u { result = first; }
        case 1u { result = second; }
        case 2u { result = first * second; }
        case 3u { result = first * second * 2.0; }
        case 4u { result = first * second * 4.0; }
        case 5u { result = first + second; }
        case 6u { result = first + second - vec4<f32>(0.5); }
        case 7u { result = first - second; }
        // Mixed by the alpha of the diffuse colour, the texture, the
        // current colour (the diffuse one again).
        case 8u, 10u { result = mix(second, first, diffuse.w); }
        case 9u { result = mix(second, first, texel.w); }
        default { result = first; }
    }
    return clamp(result, vec4<f32>(0.0), vec4<f32>(1.0));
}

fn passes(function: u32, value: u32, reference: u32) -> bool {
    switch function {
        case 1u { return false; }
        case 2u { return value < reference; }
        case 3u { return value == reference; }
        case 4u { return value <= reference; }
        case 5u { return value > reference; }
        case 6u { return value != reference; }
        case 7u { return value >= reference; }
        default { return true; }
    }
}

fn shade(diffuse: vec4<f32>, specular: vec4<f32>, uv: vec2<f32>) -> vec4<f32> {
    let texel = textureSample(texture, texture_sampler, uv);
    if draw.test.w != 0u && all(abs(texel - draw.key) < vec4<f32>(0.5 / 255.0)) {
        discard;
    }
    let color = combine(draw.color, diffuse, texel, specular).xyz;
    let alpha = combine(draw.alpha, diffuse, texel, specular).w;
    if draw.test.x != 0u && !passes(draw.test.x, u32(round(alpha * 255.0)), draw.test.y) {
        discard;
    }
    var result = color;
    if draw.test.z != 0u {
        result = min(color + specular.xyz, vec3<f32>(1.0));
    }
    // The specular alpha is the share of the colour that the fog leaves.
    if draw.fog.w != 0.0 {
        result = mix(draw.fog.xyz, result, specular.w);
    }
    return vec4<f32>(result, alpha);
}

@fragment
fn fragment_smooth(input: Smooth) -> @location(0) vec4<f32> {
    return shade(input.diffuse, input.specular, input.uv);
}

// --- Fills and copies -----------------------------------------------------
//
// Both draw one triangle that covers the viewport, which is set to the
// rectangle being written.

struct RectUniforms {
    // Fill: the colour, or the depth in `x`.
    value: vec4<f32>,
    // Copy: source and target rectangles as x, y, width, height in pixels.
    source: vec4<u32>,
    destination: vec4<u32>,
    // Copy: the first component is not zero when the source pixels equal
    // to `value` are left out.
    keyed: vec4<u32>,
}

@group(0) @binding(0) var<uniform> rect: RectUniforms;
@group(1) @binding(0) var source: texture_2d<f32>;

@vertex
fn vertex_cover(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(corner * 2.0 - vec2<f32>(1.0), rect.value.x, 1.0);
}

@fragment
fn fragment_fill() -> @location(0) vec4<f32> {
    return rect.value;
}

// A depth fill writes the depth of the triangle, set in the vertex shader.
@fragment
fn fragment_nothing() {}

// Each target pixel takes the source pixel at the same fraction of the way
// across, rounded down: the same pixels a copy in memory picks.
@fragment
fn fragment_copy(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let offset = vec2<u32>(position.xy) - rect.destination.xy;
    let pixel = rect.source.xy + offset * rect.source.zw / rect.destination.zw;
    let texel = textureLoad(source, pixel, 0);
    if rect.keyed.x != 0u && all(abs(texel - rect.value) < vec4<f32>(0.5 / 255.0)) {
        discard;
    }
    return texel;
}

// --- Showing a surface in a window ----------------------------------------

@group(0) @binding(0) var shown: texture_2d<f32>;
@group(0) @binding(1) var shown_sampler: sampler;

struct Shown {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

// One triangle that covers the viewport, with the picture across it.
@vertex
fn vertex_show(@builtin(vertex_index) index: u32) -> Shown {
    let corner = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return Shown(vec4<f32>(corner * 2.0 - vec2<f32>(1.0), 0.0, 1.0), vec2<f32>(corner.x, 1.0 - corner.y));
}

@fragment
fn fragment_show(input: Shown) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSample(shown, shown_sampler, input.uv).xyz, 1.0);
}

// --- Depth surfaces where depth cannot be copied to and from buffers -------
//
// The 16-bit values travel through a texture of unsigned numbers instead.

@group(1) @binding(0) var depth_numbers: texture_2d<u32>;
// A depth texture read as plain, unfiltered numbers: every API can load a
// texel of it that way.
@group(1) @binding(0) var depth_values: texture_2d<f32>;

// Writes the numbers of a texture, laid over the rectangle being drawn, as
// depth.
@fragment
fn fragment_depth_from_numbers(@builtin(position) position: vec4<f32>) -> @builtin(frag_depth) f32 {
    let at = vec2<u32>(position.xy) - rect.destination.xy;
    return f32(textureLoad(depth_numbers, at, 0).x) / 65535.0;
}

// Writes the depth of a rectangle of a depth surface as numbers.
@fragment
fn fragment_numbers_from_depth(@builtin(position) position: vec4<f32>) -> @location(0) u32 {
    let at = vec2<u32>(position.xy) + rect.source.xy;
    return u32(round(textureLoad(depth_values, at, 0).x * 65535.0));
}
