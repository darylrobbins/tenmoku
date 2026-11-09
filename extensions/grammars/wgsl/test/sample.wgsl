// Vertex and fragment shaders for a lit, textured mesh.
/* Block comment:
   uniforms first, then the entry points. */
enable f16;

override PI: f32 = 3.14159265;
override MAX_LIGHTS: u32 = 4u;
override scale: f32 = 1.0e0;

struct Camera {
    view_proj: mat4x4<f32>,
    position: vec3<f32>,
}

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) uv: vec2<f32>,
}

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) uv: vec2<f32>,
}

@group(0) @binding(0) var<uniform> camera: Camera;
@group(1) @binding(0) var albedo: texture_2d<f32>;
@group(1) @binding(1) var albedo_sampler: sampler;
@group(2) @binding(0) var<storage, read_write> counters: array<u32>;

var<private> shade_count: i32 = 0;

fn lambert(normal: vec3<f32>, light_dir: vec3<f32>) -> f32 {
    return max(dot(normalize(normal), light_dir), 0.0);
}

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    out.clip = camera.view_proj * vec4<f32>(in.position * scale, 1.0);
    out.normal = in.normal;
    out.uv = in.uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let base = textureSample(albedo, albedo_sampler, in.uv);
    var light = 0.0;
    for (var i: u32 = 0u; i < MAX_LIGHTS; i++) {
        light += lambert(in.normal, vec3<f32>(0.0, 1.0, 0.0)) / f32(MAX_LIGHTS);
    }
    if (base.a < 0.5) {
        discard;
    }
    switch (shade_count) {
        case 0: { light = light * 0.5; }
        default: { light = f32(0x1Fu) * 0.25; }
    }
    let lit = true && !false;
    return vec4<f32>(base.rgb * light, base.a);
}

@compute @workgroup_size(8, 8, 1)
fn cs_main(@builtin(global_invocation_id) id: vec3<u32>) {
    counters[0] = counters[0] + 1u;
    loop {
        if (id.x > 2u) { break; }
        continuing { shade_count = shade_count + 1; }
    }
}
