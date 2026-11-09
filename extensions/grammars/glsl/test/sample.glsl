#version 450 core
#extension GL_ARB_separate_shader_objects : enable
#extension GL_GOOGLE_include_directive : enable
#include "common/tone\tmap.glsl"
#define MAX_LIGHTS 8
#define SQUARE(x) ((x) * (x))

// Per-vertex inputs.
layout(location = 0) in vec3 aPosition;
layout(location = 1) in vec3 aNormal;
layout(location = 2) in vec2 aTexCoord;

layout(std140, binding = 0) uniform Camera {
    mat4 view;
    mat4 projection;
    vec3 eye;
} camera;

uniform sampler2D uAlbedo;
uniform float uTime;
uniform vec3 uLights[MAX_LIGHTS];

out vec3 vNormal;
out vec2 vTexCoord;
flat out int vLayer;

const float PI = 3.14159265;
const float EPSILON = 1e-4;
const uint SEED = 0x9E3779B9u;

struct Material {
    vec3 albedo;
    float roughness;
    bool metallic;
};

/* Schlick's approximation of the Fresnel term.
 * Used by the lighting loop below. */
float fresnel(float cosTheta, float f0) {
    return f0 + (1.0 - f0) * pow(clamp(1.0 - cosTheta, 0.0, 1.0), 5.0);
}

vec3 shade(Material m, vec3 n, vec3 v) {
    vec3 color = vec3(0.0);
    for (int i = 0; i < MAX_LIGHTS; ++i) {
        vec3 l = normalize(uLights[i] - v);
        float diff = max(dot(n, l), 0.0);
        color += m.albedo * diff * fresnel(diff, 0.04);
    }
    if (m.metallic) {
        color *= 0.5;
    } else if (m.roughness > .5) {
        color = mix(color, vec3(1.0), 0.25);
    }
    return color;
}

void main() {
    vNormal = mat3(transpose(inverse(camera.view))) * aNormal;
    vTexCoord = aTexCoord * SQUARE(2.0);
    vLayer = int(floor(uTime)) % 4;
    gl_Position = camera.projection * camera.view * vec4(aPosition, 1.0);
}
