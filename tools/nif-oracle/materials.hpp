// SPDX-License-Identifier: GPL-3.0-only
// Read-only projection of nifly's raw material objects for independent comparison.
#pragma once
#include "NifFile.hpp"
#include "Shaders.hpp"
#include <iostream>
#include <string>

namespace material_oracle {
using namespace nifly;
inline void ref(uint32_t value) { if (value == UINT32_MAX) std::cout << "null"; else std::cout << value; }
template<class Range, class Emit> void array(const Range& values, Emit emit) {
    std::cout << '['; bool first = true;
    for (auto it = values.cbegin(); it != values.cend(); ++it) {
        if (!first) std::cout << ','; first = false; emit(*it);
    }
    std::cout << ']';
}
inline void bytes(const std::string& value) { array(value, [](unsigned char c) { std::cout << unsigned(c); }); }
inline void vec3(const Vector3& v) { std::cout << '[' << v.x << ',' << v.y << ',' << v.z << ']'; }
inline void vec2(const Vector2& v) { std::cout << '[' << v.u << ',' << v.v << ']'; }
inline void object(const NiObjectNET* net) {
    if (!net) { std::cout << "null"; return; }
    std::cout << "{\"name\":"; ref(net->name.GetIndex());
    std::cout << ",\"extra_data\":"; array(net->extraDataRefs, [](const auto& r) { ref(r.index); });
    std::cout << ",\"controller\":"; ref(net->controllerRef.index); std::cout << '}';
}
inline void shader(const BSShaderLightingProperty& p) {
    std::cout << "{\"shade_flags\":" << p.shadingFlags << ",\"shader_type\":" << p.shaderType
              << ",\"flags\":" << p.shaderFlags1 << ",\"flags2\":" << p.shaderFlags2
              << ",\"environment_scale\":" << p.environmentMapScale << ",\"clamp_mode\":" << p.textureClampMode << '}';
}
inline void slot(bool present, const TexDesc& t) {
    if (!present) { std::cout << "null"; return; }
    std::cout << "{\"source\":"; ref(t.sourceRef.index);
    std::cout << ",\"flags\":" << t.flags << ",\"transform\":";
    if (t.hasTexTransform) {
        std::cout << "{\"translation\":"; vec2(t.transform.translation);
        std::cout << ",\"scale\":"; vec2(t.transform.scale);
        std::cout << ",\"rotation\":" << t.transform.wRotation << ",\"method\":" << t.transform.transformType << ",\"center\":";
        vec2(t.transform.center); std::cout << '}';
    } else std::cout << "null";
    std::cout << '}';
}
inline void optional(bool present, float value) { if (present) std::cout << value; else std::cout << "null"; }

inline void write(NiHeader& h) {
    const auto stream = h.GetVersion().Stream();
    std::cout << ",\"materials\":["; bool first = true;
    for (uint32_t id = 0; id < h.GetNumBlocks(); ++id) {
        const auto name = h.GetBlockTypeStringById(id);
        if (name != "NiMaterialProperty" && name != "NiAlphaProperty" && name != "NiStencilProperty"
            && name != "NiShadeProperty" && name != "BSShaderPPLightingProperty" && name != "BSShaderNoLightingProperty"
            && name != "BSShaderTextureSet" && name != "NiSourceTexture" && name != "NiTexturingProperty") continue;
        if (!first) std::cout << ','; first = false;
        std::cout << "{\"block\":" << id << ",\"object\":"; object(h.GetBlock<NiObjectNET>(id));
        std::cout << ",\"data\":{";
        if (name == "NiMaterialProperty") {
            auto p = h.GetBlock<NiMaterialProperty>(id);
            std::cout << "\"kind\":\"material\",\"ambient\":";
            if (stream < 26) vec3(p->colorAmbient); else std::cout << "null";
            std::cout << ",\"diffuse\":"; if (stream < 26) vec3(p->colorDiffuse); else std::cout << "null";
            std::cout << ",\"specular\":"; vec3(p->GetSpecularColor());
            auto c = p->GetEmissiveColor();
            std::cout << ",\"emissive\":[" << c.r << ',' << c.g << ',' << c.b << ']'
                      << ",\"glossiness\":" << p->GetGlossiness() << ",\"alpha\":" << p->GetAlpha() << ",\"emissive_multiplier\":";
            optional(stream > 21, p->GetEmissiveMultiple());
        } else if (name == "NiAlphaProperty") {
            auto p = h.GetBlock<NiAlphaProperty>(id);
            std::cout << "\"kind\":\"alpha\",\"flags\":" << p->flags << ",\"threshold\":" << unsigned(p->threshold);
        } else if (name == "NiStencilProperty") {
            auto p = h.GetBlock<NiStencilProperty>(id);
            std::cout << "\"kind\":\"stencil\",\"flags\":" << p->flags << ",\"reference\":" << p->stencilRef << ",\"mask\":" << p->stencilMask;
        } else if (name == "NiShadeProperty") {
            std::cout << "\"kind\":\"shade\",\"flags\":" << h.GetBlock<NiShadeProperty>(id)->shadingFlags;
        } else if (name == "BSShaderPPLightingProperty") {
            auto p = h.GetBlock<BSShaderPPLightingProperty>(id);
            std::cout << "\"kind\":\"per_pixel_lighting\",\"shader\":"; shader(*p);
            std::cout << ",\"texture_set\":"; ref(p->textureSetRef.index);
            std::cout << ",\"refraction_strength\":"; optional(stream > 14, p->refractionStrength);
            std::cout << ",\"refraction_period\":"; if (stream > 14) std::cout << p->refractionFirePeriod; else std::cout << "null";
            std::cout << ",\"parallax_passes\":"; optional(stream > 24, p->parallaxMaxPasses);
            std::cout << ",\"parallax_scale\":"; optional(stream > 24, p->parallaxScale);
        } else if (name == "BSShaderNoLightingProperty") {
            auto p = h.GetBlock<BSShaderNoLightingProperty>(id);
            std::cout << "\"kind\":\"no_lighting\",\"shader\":"; shader(*p);
            std::cout << ",\"texture\":"; bytes(p->baseTexture.get());
            std::cout << ",\"falloff\":";
            if (stream > 26) std::cout << '[' << p->falloffStartAngle << ',' << p->falloffStopAngle << ',' << p->falloffStartOpacity << ',' << p->falloffStopOpacity << ']';
            else std::cout << "null";
        } else if (name == "BSShaderTextureSet") {
            std::cout << "\"kind\":\"texture_set\",\"textures\":";
            array(h.GetBlock<BSShaderTextureSet>(id)->textures, [](const auto& t) { bytes(t.get()); });
        } else if (name == "NiSourceTexture") {
            auto p = h.GetBlock<NiSourceTexture>(id);
            std::cout << "\"kind\":\"source_texture\",\"external\":" << unsigned(p->useExternal) << ",\"filename\":";
            ref(p->fileName.GetIndex()); std::cout << ",\"pixel_data\":"; ref(p->dataRef.index);
            std::cout << ",\"format_preferences\":[" << p->pixelLayout << ',' << p->mipMapFormat << ',' << p->alphaFormat << ']'
                      << ",\"is_static\":" << unsigned(p->isStatic) << ",\"direct_render\":" << (p->directRender ? "true" : "false")
                      << ",\"persist_render_data\":" << (p->persistentRenderData ? "true" : "false");
        } else {
            auto p = h.GetBlock<NiTexturingProperty>(id);
            std::cout << "\"kind\":\"texturing\",\"flags\":" << p->flags << ",\"texture_count\":" << p->textureCount << ",\"slots\":[";
            const bool used[] = {p->hasBaseTex, p->hasDarkTex, p->hasDetailTex, p->hasGlossTex, p->hasGlowTex, p->hasBumpTex,
                p->hasNormalTex, p->hasParallaxTex, p->hasDecalTex0, p->hasDecalTex1, p->hasDecalTex2, p->hasDecalTex3};
            const TexDesc* slots[] = {&p->baseTex, &p->darkTex, &p->detailTex, &p->glossTex, &p->glowTex, &p->bumpTex,
                &p->normalTex, &p->parallaxTex, &p->decalTex0, &p->decalTex1, &p->decalTex2, &p->decalTex3};
            for (uint32_t i = 0; i < std::min(12u, std::max(5u, p->textureCount)); ++i) { if (i) std::cout << ','; slot(used[i], *slots[i]); }
            std::cout << "],\"bump_luma\":";
            if (p->hasBumpTex) std::cout << '[' << p->lumaScale << ',' << p->lumaOffset << ']'; else std::cout << "null";
            std::cout << ",\"bump_matrix\":";
            if (p->hasBumpTex) std::cout << '[' << p->bumpMatrix.x << ',' << p->bumpMatrix.y << ',' << p->bumpMatrix.z << ',' << p->bumpMatrix.w << ']'; else std::cout << "null";
            std::cout << ",\"parallax_offset\":"; optional(p->hasParallaxTex, p->parallaxOffset);
            std::cout << ",\"shader_textures\":";
            array(p->shaderTex, [](const ShaderTexDesc& t) {
                if (!t.isUsed) { std::cout << "null"; return; }
                std::cout << "{\"texture\":"; slot(true, t.data); std::cout << ",\"map_id\":" << t.mapIndex << '}';
            });
        }
        std::cout << "}}";
    }
    std::cout << ']';
}
} // namespace material_oracle
