// A lit, textured forward pass with a compute post-process.
#include "common.hlsli"

#define MAX_LIGHTS 4
#define SATURATE(x) clamp((x), 0.0, 1.0)

/** Per-frame constants, updated once a frame. */
cbuffer FrameConstants
{
    row_major float4x4 ViewProjection;
    float3 CameraPosition;
    float Time;
    uint LightCount;
};

struct PointLight
{
    float3 position;
    float radius;
    float3 color;
    float intensity;
};

StructuredBuffer<PointLight> Lights : register(t1);
Texture2D<float4> AlbedoMap : register(t0);
SamplerState LinearSampler : register(s0);
RWTexture2D<float4> Output : register(u0);
groupshared float4 TileCache[64];

struct VSInput
{
    float3 position : POSITION;
    float3 normal : NORMAL;
    float2 uv : TEXCOORD0;
};

struct PSInput
{
    float4 position : SV_Position;
    float3 worldPos : TEXCOORD1;
    nointerpolation float3 normal : NORMAL;
    float2 uv : TEXCOORD0;
};

static const float PI = 3.14159265f;
static const uint TILE_SIZE = 0x10;

float Attenuate(float distance, float radius)
{
    float ratio = saturate(1.0 - pow(distance / radius, 4));
    return ratio * ratio / (distance * distance + 1e-4);
}

PSInput VSMain(VSInput input)
{
    PSInput output;
    output.worldPos = input.position;
    output.position = mul(float4(input.position, 1.0f), ViewProjection);
    output.normal = normalize(input.normal);
    output.uv = input.uv;
    return output;
}

float4 PSMain(PSInput input) : SV_Target
{
    float4 albedo = AlbedoMap.Sample(LinearSampler, input.uv);
    if (albedo.a < 0.5)
    {
        discard;
    }

    float3 lit = float3(0.03, 0.03, 0.03) * albedo.rgb;
    [unroll]
    for (uint i = 0; i < MAX_LIGHTS; ++i)
    {
        if (i >= LightCount)
            break;
        PointLight light = Lights[i];
        float3 toLight = light.position - input.worldPos;
        float distance = length(toLight);
        float diffuse = max(dot(input.normal, toLight / distance), 0.0);
        lit += diffuse * light.color * light.intensity * Attenuate(distance, light.radius);
    }
    return float4(SATURATE(lit), albedo.a);
}

[numthreads(8, 8, 1)]
void CSMain(uint3 id : SV_DispatchThreadID, uint index : SV_GroupIndex)
{
    TileCache[index] = Output[id.xy];
    GroupMemoryBarrierWithGroupSync();

    precise float vignette = 1.0 - 0.25 * length(float2(id.xy) / 1024.0 - 0.5);
    uint row = uint(id.y) / TILE_SIZE;
    bool flicker = sin(Time * PI) > 0.99 ? true : false;
    Output[id.xy] = TileCache[index] * (flicker ? 0.9 : vignette);

    /* Shader Model 6 printf, for debugging under PIX. */
    if (all(id.xy == 0))
        printf("tile \"%u\" done\n", index);
}
