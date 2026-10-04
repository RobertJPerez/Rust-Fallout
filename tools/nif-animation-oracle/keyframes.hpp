// SPDX-License-Identifier: GPL-3.0-only
// Raw count/tag admission supplements the pinned factory. In particular nifly
// discards the leading XYZ count and uses NO_INTERP for absent source tags.
struct KeyGroupCounts { uint32_t count = 0, tag = 0; };
struct KeyCounts {
    uint32_t rotations = 0, rotation_tag = 0;
    KeyGroupCounts x, y, z, translation, scale;
};
static void key_work(size_t& remaining, size_t amount) {
    if (amount > remaining) throw std::runtime_error("native transform-key work budget exceeded");
    remaining -= amount;
}
static bool admitted_key_tag(uint32_t tag) { return tag == 1 || tag == 2 || tag == 3 || tag == 5; }
template<class T>
static KeyGroupCounts admit_group(Scan& input, size_t channels, size_t& remaining, size_t& work) {
    key_work(work, 1);
    KeyGroupCounts result; result.count = input.integer();
    if (!result.count) return result;
    result.tag = input.integer();
    if (!admitted_key_tag(result.tag)) throw std::runtime_error("unadmitted native transform key-group tag");
    const size_t words = 1 + channels + (result.tag == 2 ? 2 * channels : result.tag == 3 ? 3 : 0);
    input.count(result.count, 4 * words);
    key_work(work, size_t(result.count) * words);
    storage(remaining, result.count, sizeof(NiAnimationKey<T>));
    for (size_t i = 0; i < size_t(result.count) * words; ++i) { finite_at(input.bytes, input.position); input.take(4); }
    return result;
}
static KeyCounts preflight_keys(std::string_view bytes, size_t& remaining, size_t& work) {
    key_work(work, 1); storage(remaining, 1, sizeof(NiTransformData));
    Scan input{bytes}; KeyCounts result; result.rotations = input.integer(); key_work(work, 1);
    if (result.rotations) {
        result.rotation_tag = input.integer();
        if (result.rotation_tag == 4) {
            if (result.rotations != 1) throw std::runtime_error("XYZ rotation source count other than one is unadmitted");
            result.x = admit_group<float>(input, 1, remaining, work);
            result.y = admit_group<float>(input, 1, remaining, work);
            result.z = admit_group<float>(input, 1, remaining, work);
        } else {
            if (!admitted_key_tag(result.rotation_tag)) throw std::runtime_error("unadmitted native quaternion key tag");
            const size_t words = result.rotation_tag == 3 ? 8 : 5;
            input.count(result.rotations, words * 4);
            key_work(work, size_t(result.rotations) * words);
            storage(remaining, result.rotations, sizeof(NiAnimationKey<Quaternion>));
            for (size_t i = 0; i < size_t(result.rotations) * words; ++i) { finite_at(bytes, input.position); input.take(4); }
        }
    }
    result.translation = admit_group<Vector3>(input, 3, remaining, work);
    result.scale = admit_group<float>(input, 1, remaining, work);
    input.end(); return result;
}
static void key_value(std::ostream& out, float v) { out << '[' << bits(v) << ']'; }
static void key_value(std::ostream& out, const Vector3& v) { vec(out, v); }
static void tbc(std::ostream& out, const TBC& v) { out << '[' << bits(v.tension) << ',' << bits(v.bias) << ',' << bits(v.continuity) << ']'; }
template<class T>
static void project_group(std::ostream& out, const NiAnimationKeyGroup<T>& group, KeyGroupCounts raw) {
    if (group.GetNumKeys() != raw.count || (raw.count && static_cast<uint32_t>(group.GetInterpolationType()) != raw.tag))
        throw std::runtime_error("native key-group count/tag differs from original source");
    out << "{\"declared_keys\":" << raw.count << ",\"key_type\":";
    if (raw.count) out << raw.tag; else out << "null";
    out << ",\"keys\":[";
    for (uint32_t i = 0; i < raw.count; ++i) {
        if (i) out << ',';
        const auto key = group.GetKey(static_cast<int>(i));
        out << "{\"time_bits\":" << bits(key.time) << ",\"value_bits\":"; key_value(out, key.value);
        out << ",\"forward_bits\":"; if (raw.tag == 2) key_value(out, key.forward); else out << "null";
        out << ",\"backward_bits\":"; if (raw.tag == 2) key_value(out, key.backward); else out << "null";
        out << ",\"tbc_bits\":"; if (raw.tag == 3) tbc(out, key.tbc); else out << "null";
        out << '}';
    }
    out << "]}";
}
static void project_keys(std::ostream& out, const NiTransformData& data, const KeyCounts& raw) {
    if (raw.rotations && static_cast<uint32_t>(data.rotationType) != raw.rotation_tag)
        throw std::runtime_error("native rotation tag differs from source");
    out << "{\"declared_rotation_keys\":" << raw.rotations << ",\"rotation\":{\"layout\":";
    if (!raw.rotations) text(out, "absent");
    else if (raw.rotation_tag == 4) {
        text(out, "xyz"); out << ",\"axes\":[";
        project_group(out, data.xRotations, raw.x); out << ',';
        project_group(out, data.yRotations, raw.y); out << ',';
        project_group(out, data.zRotations, raw.z); out << ']';
    } else {
        if (data.quaternionKeys.size() != raw.rotations) throw std::runtime_error("native quaternion count differs from source");
        text(out, "quaternion"); out << ",\"key_type\":" << raw.rotation_tag << ",\"keys\":[";
        for (size_t i = 0; i < data.quaternionKeys.size(); ++i) {
            if (i) out << ','; const auto& key = data.quaternionKeys[i];
            out << "{\"time_bits\":" << bits(key.time) << ",\"value_wxyz_bits\":[" << bits(key.value.w) << ',' << bits(key.value.x) << ',' << bits(key.value.y) << ',' << bits(key.value.z) << "],\"tbc_bits\":";
            if (raw.rotation_tag == 3) tbc(out, key.tbc); else out << "null"; out << '}';
        }
        out << ']';
    }
    out << "},\"translations\":"; project_group(out, data.translations, raw.translation);
    out << ",\"scales\":"; project_group(out, data.scales, raw.scale); out << '}';
}
