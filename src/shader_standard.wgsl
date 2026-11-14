struct Camera {
    view_proj: mat4x4<f32>,
};

struct Model {
    model: mat4x4<f32>,
    normal_matrix: mat4x4<f32>,
};
struct Light { view_proj: mat4x4<f32>, params: vec4<f32>, direction: vec4<f32>, };
struct Material { color: vec4<f32>, };
@group(3) @binding(0) var<uniform> material: Material;
@group(2) @binding(0) var<uniform> light: Light;
@group(2) @binding(1) var shadow_map: texture_depth_2d;
@group(2) @binding(2) var shadow_sampler: sampler_comparison;

@group(0) @binding(0)
var<uniform> camera: Camera;

@group(1) @binding(0)
var<uniform> model_data: Model;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec3<f32>,
    @location(1) shadow_pos: vec4<f32>,
    @location(2) normal: vec3<f32>,
};

@vertex
fn vs_main(
    @location(0) position: vec3<f32>,
    @location(1) color: vec3<f32>,
    @location(2) normal: vec3<f32>,
) -> VertexOutput {
    var output: VertexOutput;
    output.position = camera.view_proj * model_data.model * vec4<f32>(position, 1.0);
    output.color = color;
    output.shadow_pos = light.view_proj * model_data.model * vec4<f32>(position, 1.0);
    output.normal = normalize((model_data.normal_matrix * vec4<f32>(normal, 0.0)).xyz);
    return output;
}

@vertex
fn vs_shadow(@location(0) position: vec3<f32>, @location(1) color: vec3<f32>, @location(2) normal: vec3<f32>) -> @builtin(position) vec4<f32> {
    return light.view_proj * model_data.model * vec4<f32>(position, 1.0);
}

@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    let projected = input.shadow_pos.xyz / input.shadow_pos.w;
    let uv = projected.xy * vec2<f32>(0.5, -0.5) + vec2<f32>(0.5);
    var visibility = 1.0;
    if (all(uv >= vec2<f32>(0.0)) && all(uv <= vec2<f32>(1.0))) {
        let texel = 1.0 / 2048.0;
        visibility = 0.25 * (textureSampleCompare(shadow_map, shadow_sampler, uv + vec2<f32>(-texel, -texel), projected.z - 0.002) + textureSampleCompare(shadow_map, shadow_sampler, uv + vec2<f32>(texel, -texel), projected.z - 0.002) + textureSampleCompare(shadow_map, shadow_sampler, uv + vec2<f32>(-texel, texel), projected.z - 0.002) + textureSampleCompare(shadow_map, shadow_sampler, uv + vec2<f32>(texel, texel), projected.z - 0.002));
    }
    let light_direction = normalize(-light.direction.xyz);
    let diffuse = max(dot(input.normal, light_direction), 0.0);
    let lighting = light.params.x + light.params.y * diffuse * visibility;
    return vec4<f32>(input.color * lighting * material.color.xyz, material.color.w);
}
