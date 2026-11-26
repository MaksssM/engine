struct Camera {
    view_proj: mat4x4<f32>,
    camera_pos: vec4<f32>,
};

struct Model {
    model: mat4x4<f32>,
    normal_matrix: mat4x4<f32>,
};

struct Light {
    view_proj: mat4x4<f32>,
    params: vec4<f32>,
    direction: vec4<f32>,
};

struct PbrMaterial {
    base_color_factor: vec4<f32>,
    emissive_and_cutoff: vec4<f32>,
    pbr_factors: vec4<f32>,
    flags: vec4<u32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;

@group(1) @binding(0) var<uniform> model_data: Model;

@group(2) @binding(0) var<uniform> light: Light;
@group(2) @binding(1) var shadow_map: texture_depth_2d;
@group(2) @binding(2) var shadow_sampler: sampler_comparison;

@group(3) @binding(0) var<uniform> material: PbrMaterial;
@group(3) @binding(1) var base_color_tex: texture_2d<f32>;
@group(3) @binding(2) var base_color_sampler: sampler;
@group(3) @binding(3) var metallic_roughness_tex: texture_2d<f32>;
@group(3) @binding(4) var metallic_roughness_sampler: sampler;
@group(3) @binding(5) var normal_tex: texture_2d<f32>;
@group(3) @binding(6) var normal_sampler: sampler;
@group(3) @binding(7) var occlusion_tex: texture_2d<f32>;
@group(3) @binding(8) var occlusion_sampler: sampler;
@group(3) @binding(9) var emissive_tex: texture_2d<f32>;
@group(3) @binding(10) var emissive_sampler: sampler;

struct VertexInput {
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) tangent: vec4<f32>,
};

struct InstanceInput {
    @location(5) model_0: vec4<f32>,
    @location(6) model_1: vec4<f32>,
    @location(7) model_2: vec4<f32>,
    @location(8) model_3: vec4<f32>,
    @location(9) normal_0: vec4<f32>,
    @location(10) normal_1: vec4<f32>,
    @location(11) normal_2: vec4<f32>,
    @location(12) normal_3: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) shadow_pos: vec4<f32>,
    @location(2) normal: vec3<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) world_pos: vec3<f32>,
    @location(5) tangent: vec4<f32>,
};

@vertex
fn vs_main(input: VertexInput) -> VertexOutput {
    var output: VertexOutput;
    let world_pos4 = model_data.model * vec4<f32>(input.position, 1.0);
    output.position = camera.view_proj * world_pos4;
    output.color = input.color;
    output.shadow_pos = light.view_proj * world_pos4;
    output.normal = normalize((model_data.normal_matrix * vec4<f32>(input.normal, 0.0)).xyz);
    output.uv = input.uv;
    output.world_pos = world_pos4.xyz;
    let world_tangent = normalize((model_data.normal_matrix * vec4<f32>(input.tangent.xyz, 0.0)).xyz);
    output.tangent = vec4<f32>(world_tangent, input.tangent.w);
    return output;
}

@vertex
fn vs_shadow(input: VertexInput) -> @builtin(position) vec4<f32> {
    return light.view_proj * model_data.model * vec4<f32>(input.position, 1.0);
}

@vertex
fn vs_instanced(input: VertexInput, inst: InstanceInput) -> VertexOutput {
    let model = mat4x4<f32>(inst.model_0, inst.model_1, inst.model_2, inst.model_3);
    let normal_matrix = mat4x4<f32>(inst.normal_0, inst.normal_1, inst.normal_2, inst.normal_3);
    var output: VertexOutput;
    let world_pos4 = model * vec4<f32>(input.position, 1.0);
    output.position = camera.view_proj * world_pos4;
    output.color = input.color;
    output.shadow_pos = light.view_proj * world_pos4;
    output.normal = normalize((normal_matrix * vec4<f32>(input.normal, 0.0)).xyz);
    output.uv = input.uv;
    output.world_pos = world_pos4.xyz;
    let world_tangent = normalize((normal_matrix * vec4<f32>(input.tangent.xyz, 0.0)).xyz);
    output.tangent = vec4<f32>(world_tangent, input.tangent.w);
    return output;
}

const PI: f32 = 3.14159265359;

fn distribution_ggx(NdotH: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let a2 = a * a;
    let NdotH2 = NdotH * NdotH;
    let denom = NdotH2 * (a2 - 1.0) + 1.0;
    return a2 / (PI * denom * denom);
}

fn visibility_smith(NdotV: f32, NdotL: f32, roughness: f32) -> f32 {
    let a = roughness * roughness;
    let gv = NdotL * (NdotV * (1.0 - a) + a);
    let gl = NdotV * (NdotL * (1.0 - a) + a);
    return 0.5 / max(gv + gl, 0.00001);
}

fn fresnel_schlick(VdotH: f32, f0: vec3<f32>) -> vec3<f32> {
    return f0 + (vec3<f32>(1.0) - f0) * pow(clamp(1.0 - VdotH, 0.0, 1.0), 5.0);
}

@fragment
fn fs_main(input: VertexOutput, @builtin(front_facing) is_front: bool) -> @location(0) vec4<f32> {
    var N = normalize(input.normal);
    if (material.flags.z == 1u && !is_front) {
        N = -N;
    }

    if ((material.flags.x & 4u) != 0u && length(input.tangent.xyz) > 0.01) {
        let T = normalize(input.tangent.xyz - dot(input.tangent.xyz, N) * N);
        let handedness = select(input.tangent.w, 1.0, abs(input.tangent.w) < 0.5);
        let B = cross(N, T) * handedness;
        let normal_raw = textureSample(normal_tex, normal_sampler, input.uv).xyz * 2.0 - 1.0;
        N = normalize(T * normal_raw.x + B * normal_raw.y + N * normal_raw.z);
    }

    var base_color = material.base_color_factor;
    if ((material.flags.x & 1u) != 0u) {
        base_color = base_color * textureSample(base_color_tex, base_color_sampler, input.uv);
    }
    base_color = base_color * vec4<f32>(input.color, 1.0);

    let alpha_mode = material.flags.y;
    if (alpha_mode == 1u && base_color.a < material.emissive_and_cutoff.w) {
        discard;
    }
    var out_alpha = base_color.a;
    if (alpha_mode == 0u) {
        out_alpha = 1.0;
    }

    var metallic = material.pbr_factors.x;
    var roughness = material.pbr_factors.y;
    if ((material.flags.x & 2u) != 0u) {
        let mr_sample = textureSample(metallic_roughness_tex, metallic_roughness_sampler, input.uv);
        metallic = clamp(metallic * mr_sample.b, 0.0, 1.0);
        roughness = clamp(roughness * mr_sample.g, 0.04, 1.0);
    } else {
        roughness = clamp(roughness, 0.04, 1.0);
    }

    var occlusion = 1.0;
    if ((material.flags.x & 8u) != 0u) {
        occlusion = textureSample(occlusion_tex, occlusion_sampler, input.uv).r;
    }

    var emissive = material.emissive_and_cutoff.xyz;
    if ((material.flags.x & 16u) != 0u) {
        emissive = emissive * textureSample(emissive_tex, emissive_sampler, input.uv).xyz;
    }

    let V = normalize(camera.camera_pos.xyz - input.world_pos);
    let L = normalize(-light.direction.xyz);
    let H = normalize(V + L);

    let NdotV = max(dot(N, V), 0.0001);
    let NdotL = max(dot(N, L), 0.0);
    let NdotH = max(dot(N, H), 0.0);
    let VdotH = max(dot(V, H), 0.0);

    let f0 = mix(vec3<f32>(0.04), base_color.rgb, metallic);

    let D = distribution_ggx(NdotH, roughness);
    let Vis = visibility_smith(NdotV, NdotL, roughness);
    let F = fresnel_schlick(VdotH, f0);

    let specular = D * Vis * F;
    let kd = (vec3<f32>(1.0) - F) * (1.0 - metallic);
    let diffuse = kd * (base_color.rgb / PI);

    let projected = input.shadow_pos.xyz / input.shadow_pos.w;
    let s_uv = projected.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    var shadow_visibility = 1.0;
    if (all(s_uv >= vec2<f32>(0.0)) && all(s_uv <= vec2<f32>(1.0))) {
        let texel = 1.0 / 2048.0;
        shadow_visibility = 0.25 * (
            textureSampleCompare(shadow_map, shadow_sampler, s_uv + vec2<f32>(-texel, -texel), projected.z - 0.002) +
            textureSampleCompare(shadow_map, shadow_sampler, s_uv + vec2<f32>(texel, -texel), projected.z - 0.002) +
            textureSampleCompare(shadow_map, shadow_sampler, s_uv + vec2<f32>(-texel, texel), projected.z - 0.002) +
            textureSampleCompare(shadow_map, shadow_sampler, s_uv + vec2<f32>(texel, texel), projected.z - 0.002)
        );
    }

    let ambient = vec3<f32>(0.05) * base_color.rgb * occlusion;
    let direct = (diffuse + specular) * light.params.xyz * NdotL * shadow_visibility * PI;
    let final_rgb = ambient + direct + emissive;

    return vec4<f32>(final_rgb, out_alpha);
}
