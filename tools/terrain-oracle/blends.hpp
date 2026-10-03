// Original offline calculation. Read sparse source samples, then evaluate each
// vertex independently; no runtime blend implementation is called or copied.
#pragma once
#include <array>

struct BlendInput {
    size_t source_layer;
    unsigned texture;
    bool alpha_missing = true;
    std::array<unsigned char, 289> weights{};
    std::array<bool, 289> seen{};
};

static std::string blend_maps(const Bytes& bytes) {
    if (signature(bytes, 0) != "LAND") return "null";
    std::array<std::optional<BlendInput>, 4> bases;
    std::array<std::vector<BlendInput>, 4> overlays;
    std::optional<size_t> extended;
    BlendInput* active = nullptr;
    size_t index = 0, clamped = 0, defaults = 0;
    for (size_t pos = 4; pos < bytes.size();) {
        if (bytes.size() - pos < 6) throw std::runtime_error("blend field header");
        const auto kind = signature(bytes, pos);
        size_t size = number(bytes, pos + 4, 2); pos += 6;
        if (kind == "XXXX") {
            if (size != 4 || extended || bytes.size() - pos < 4) throw std::runtime_error("blend XXXX");
            extended = number(bytes, pos, 4); pos += 4; continue;
        }
        if (extended) { size = *extended; extended.reset(); }
        if (size > bytes.size() - pos) throw std::runtime_error("blend field overrun");
        const size_t start = pos; pos += size;
        if (kind == "BTXT" || kind == "ATXT") {
            if (size != 8 || bytes[start + 4] > 3 || index >= 256) throw std::runtime_error("blend layer extent");
            const unsigned q = bytes[start + 4];
            BlendInput layer{index++, number(bytes, start, 4)};
            defaults += layer.texture == 0;
            if (kind == "BTXT") {
                if (bases[q]) throw std::runtime_error("blend duplicate base");
                layer.weights.fill(255); bases[q] = layer; active = nullptr;
            } else {
                if (signed_number(bytes, start + 6, 2) != overlays[q].size()) throw std::runtime_error("blend layer order");
                overlays[q].push_back(layer); active = &overlays[q].back();
            }
        } else if (kind == "VTXT") {
            if (!active || !active->alpha_missing || size % 8 || size / 8 > 289) throw std::runtime_error("blend alpha extent");
            active->alpha_missing = false;
            for (size_t i = 0; i < size; i += 8) {
                const unsigned at = number(bytes, start + i, 2);
                if (at >= 289 || active->seen[at]) throw std::runtime_error("blend duplicate or invalid sample");
                active->seen[at] = true;
                const uint32_t bits = number(bytes, start + i + 4, 4); float opacity;
                std::memcpy(&opacity, &bits, 4);
                if (!std::isfinite(opacity)) throw std::runtime_error("blend nonfinite sample");
                clamped += opacity < 0.f || opacity > 1.f;
                const float scaled = std::clamp(opacity, 0.f, 1.f) * 255.f;
                active->weights[at] = static_cast<unsigned char>(scaled);
            }
        }
    }
    if (extended) throw std::runtime_error("blend orphan XXXX");
    const auto project = [](const BlendInput& layer) {
        std::vector<std::string> weights;
        for (auto weight : layer.weights) weights.push_back(std::to_string(weight));
        return object({{"source_layer", std::to_string(layer.source_layer)}, {"texture_raw", std::to_string(layer.texture)},
            {"alpha_missing", layer.alpha_missing ? "true" : "false"}, {"weights", array(weights)}});
    };
    std::vector<std::string> quadrants, missing;
    size_t overfull = 0;
    for (unsigned q = 0; q < 4; ++q) {
        if (!bases[q]) missing.push_back(std::to_string(q));
        for (unsigned vertex = 0; vertex < 289; ++vertex) {
            unsigned coverage = 0;
            for (const auto& overlay : overlays[q]) coverage += overlay.weights[vertex];
            overfull += coverage > 255;
            if (bases[q]) bases[q]->weights[vertex] = static_cast<unsigned char>(coverage < 255 ? 255 - coverage : 0);
        }
        std::vector<std::string> layers;
        for (const auto& layer : overlays[q]) layers.push_back(project(layer));
        quadrants.push_back(object({{"quadrant", std::to_string(q)}, {"base", bases[q] ? project(*bases[q]) : "null"}, {"overlays", array(layers)}}));
    }
    return object({{"model", quote("esm4-local-u8-residual-base-v1")}, {"quadrants", array(quadrants)},
        {"clamped_samples", std::to_string(clamped)}, {"overfull_vertices", std::to_string(overfull)},
        {"unapplied_default_layers", std::to_string(defaults)}, {"missing_base_quadrants", array(missing)}});
}
