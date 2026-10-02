// SPDX-License-Identifier: GPL-3.0-only
// Offline nifly projection. Stored float bits avoid decimal rounding and signed-zero
// ambiguity. This Windows oracle is not linked into the Rust runtime.
#pragma once
#include "scene.hpp"
#include "bhk.hpp"
#include <windows.h>
#include <bcrypt.h>
#include <sstream>

namespace collision_oracle {
using namespace nifly;
using scene_oracle::ref;
using scene_oracle::array;
using scene_oracle::refs;

inline uint32_t bits(float value) { uint32_t result; std::memcpy(&result, &value, 4); return result; }
template<class V> void vector_bits(const V& value, int count) {
    std::cout << '[';
    for (int i = 0; i < count; ++i) { if (i) std::cout << ','; std::cout << bits(value[i]); }
    std::cout << ']';
}
inline void vector_bits(const Vector4& value, int count) {
    const float values[] = {value.x, value.y, value.z, value.w};
    vector_bits(values, count);
}
inline void vector_bits(const QuaternionXYZW& value, int count) {
    const float values[] = {value.x, value.y, value.z, value.w};
    vector_bits(values, count);
}
inline std::string sha256(const char* data, size_t size) {
    BCRYPT_ALG_HANDLE algorithm = nullptr;
    if (BCryptOpenAlgorithmProvider(&algorithm, BCRYPT_SHA256_ALGORITHM, nullptr, 0) < 0)
        throw std::runtime_error("CNG SHA256 provider failed");
    unsigned char digest[32]{};
    const auto status = BCryptHash(algorithm, nullptr, 0,
        reinterpret_cast<PUCHAR>(const_cast<char*>(data)), static_cast<ULONG>(size), digest, 32);
    BCryptCloseAlgorithmProvider(algorithm, 0);
    if (status < 0) throw std::runtime_error("CNG SHA256 computation failed");
    std::ostringstream result;
    result << std::hex << std::setfill('0');
    for (auto byte : digest) result << std::setw(2) << unsigned(byte);
    return result.str();
}
inline std::string file_sha256(const std::filesystem::path& path) {
    std::ifstream input(path, std::ios::binary | std::ios::ate);
    const auto size = input.tellg();
    if (!input || size < 0 || size > 64 * 1024 * 1024) throw std::runtime_error("oracle binary digest input exceeds budget");
    std::string bytes(static_cast<size_t>(size), '\0');
    input.seekg(0); input.read(bytes.data(), size);
    if (!input) throw std::runtime_error("oracle binary digest input read failed");
    return sha256(bytes.data(), bytes.size());
}
inline void filter(const HavokFilter& f) {
    std::cout << "{\"layer\":" << unsigned(f.layer) << ",\"flags_and_parts\":" << unsigned(f.flagsAndParts) << ",\"group\":" << f.group << '}';
}
inline void property(const hkWorldObjCInfoProperty& p) {
    std::cout << "{\"data\":" << p.data << ",\"size\":" << p.size << ",\"capacity_and_flags\":" << p.capacityAndFlags << '}';
}
inline uint32_t word(const char* data) { uint32_t result; std::memcpy(&result, data, 4); return result; }
inline void byte_span(const char* data, int count) {
    std::cout << '['; for (int i = 0; i < count; ++i) { if (i) std::cout << ','; std::cout << unsigned(static_cast<unsigned char>(data[i])); } std::cout << ']';
}
inline bool supported(const std::string& name) {
    return name == "bhkCollisionObject" || name == "bhkBlendCollisionObject" || name == "bhkPCollisionObject" || name == "bhkSPCollisionObject"
        || name == "bhkRigidBody" || name == "bhkRigidBodyT" || name == "bhkSphereShape" || name == "bhkBoxShape"
        || name == "bhkCapsuleShape" || name == "bhkConvexVerticesShape" || name == "bhkTransformShape"
        || name == "bhkConvexTransformShape" || name == "bhkListShape" || name == "bhkMoppBvTreeShape"
        || name == "bhkPackedNiTriStripsShape" || name == "hkPackedNiTriStripsData";
}

inline void body(const bhkRigidBody& b, bool active) {
    std::cout << "{\"kind\":\"rigid_body\",\"body\":{\"transform_active\":" << (active ? "true" : "false")
        << ",\"world\":{\"shape\":"; ref(b.shapeRef.index);
    std::cout << ",\"filter\":"; filter(b.collisionFilter);
    std::cout << ",\"unused\":" << static_cast<uint32_t>(static_cast<const bhkWorldObject&>(b).unkInt1)
        << ",\"broad_phase\":" << unsigned(b.broadPhaseType) << ",\"padding\":";
    byte_span(reinterpret_cast<const char*>(b.unkBytes), 3);
    std::cout << ",\"property\":"; property(b.prop);
    std::cout << "},\"entity_response\":" << unsigned(b.collisionResponse) << ",\"entity_unused\":" << unsigned(b.unusedByte1)
        << ",\"entity_callback_delay\":" << b.processContactCallbackDelay << ",\"unused_01\":" << b.unkInt1 << ",\"filter_copy\":";
    filter(b.collisionFilterCopy);
    const auto* padding = reinterpret_cast<const char*>(b.unkShorts2);
    std::cout << ",\"unused_02\":" << word(padding) << ",\"collision_response\":" << unsigned(static_cast<unsigned char>(padding[4]))
        << ",\"unused_03\":" << unsigned(static_cast<unsigned char>(padding[5])) << ",\"callback_delay\":" << b.unkShorts2[3]
        << ",\"unused_04\":" << word(padding + 8) << ",\"translation\":"; vector_bits(b.translation, 4);
    std::cout << ",\"rotation\":"; vector_bits(b.rotation, 4);
    std::cout << ",\"linear_velocity\":"; vector_bits(b.linearVelocity, 4);
    std::cout << ",\"angular_velocity\":"; vector_bits(b.angularVelocity, 4);
    std::cout << ",\"inertia\":[";
    for (int row = 0; row < 3; ++row) { if (row) std::cout << ','; vector_bits(b.inertiaMatrix + row * 4, 3); }
    std::cout << "],\"inertia_padding\":[" << bits(b.inertiaMatrix[3]) << ',' << bits(b.inertiaMatrix[7]) << ',' << bits(b.inertiaMatrix[11]) << ']';
    std::cout << ",\"center\":"; vector_bits(b.center, 4);
    std::cout << ",\"mass\":" << bits(b.mass) << ",\"linear_damping\":" << bits(b.linearDamping)
        << ",\"angular_damping\":" << bits(b.angularDamping) << ",\"friction\":" << bits(b.friction)
        << ",\"restitution\":" << bits(b.restitution) << ",\"max_linear_velocity\":" << bits(b.maxLinearVelocity)
        << ",\"max_angular_velocity\":" << bits(b.maxAngularVelocity) << ",\"penetration_depth\":" << bits(b.penetrationDepth)
        << ",\"motion_system\":" << unsigned(b.motionSystem) << ",\"deactivator\":" << unsigned(b.deactivatorType)
        << ",\"solver_deactivation\":" << unsigned(b.solverDeactivation) << ",\"quality\":" << unsigned(b.qualityType)
        << ",\"unused_05\":[" << b.unusedInts1[0] << ',' << b.unusedInts1[1] << ',' << b.unusedInts1[2] << "],\"constraints\":";
    refs(b.constraintRefs); std::cout << ",\"flags\":" << b.bodyFlagsInt << "}}";
}

inline void write(const std::filesystem::path& path) {
    std::ifstream file(path, std::ios::binary | std::ios::ate);
    const auto length = file.tellg();
    if (!file || length < 0 || length > 64 * 1024 * 1024) throw std::runtime_error("oracle input exceeds byte budget");
    std::string snapshot(static_cast<size_t>(length), '\0');
    file.seekg(0); file.read(snapshot.data(), length);
    if (!file) throw std::runtime_error("oracle could not snapshot input");
    std::istringstream input(snapshot, std::ios::binary);
    NiHeader header; NiIStream stream(&input, &header); header.Get(stream);
    if (!header.IsValid() || header.GetVersion().File() != V20_2_0_7 || header.GetVersion().User() != 11
        || header.GetVersion().Stream() < 14 || header.GetVersion().Stream() > 34)
        throw std::runtime_error("collision oracle requires the NV container tuple");
    std::vector<std::unique_ptr<NiObject>> blocks;
    std::vector<size_t> offsets;
    for (uint32_t id = 0; id < header.GetNumBlocks(); ++id) {
        const auto start = input.tellg(); offsets.push_back(static_cast<size_t>(start));
        auto factory = NiFactoryRegister::Get().GetFactoryByName(header.GetBlockTypeStringById(id));
        if (factory) blocks.push_back(factory->Load(stream));
        else blocks.push_back(std::make_unique<NiUnknown>(stream, header.GetBlockSize(id)));
        if (!input || input.tellg() - start != header.GetBlockSize(id)) throw std::runtime_error("collision oracle factory consumed a wrong block size");
    }
    header.GetFooter(stream);
    if (!input || input.peek() != std::char_traits<char>::eof()) throw std::runtime_error("collision oracle footer did not consume the input");
    header.SetBlockReference(&blocks);
    std::cout << "{\"file\":" << std::quoted(path.filename().string()) << ",\"sha256\":" << std::quoted(sha256(snapshot.data(), snapshot.size()))
        << ",\"decoded_bytes\":" << snapshot.size() << ",\"version\":" << static_cast<uint32_t>(header.GetVersion().File())
        << ",\"user_version\":" << header.GetVersion().User() << ",\"bethesda_version\":" << header.GetVersion().Stream() << ",\"collisions\":[";
    bool first = true;
    for (uint32_t id = 0; id < blocks.size(); ++id) {
        const auto name = header.GetBlockTypeStringById(id);
        if (!supported(name)) continue;
        if (!first) std::cout << ','; first = false;
        const auto* raw = snapshot.data() + offsets[id];
        std::cout << "{\"block\":" << id << ",\"block_type\":" << std::quoted(name) << ",\"source_offset\":" << offsets[id]
            << ",\"source_bytes\":" << header.GetBlockSize(id) << ",\"source_sha256\":" << std::quoted(sha256(raw, header.GetBlockSize(id))) << ",\"data\":";
        if (auto* p = header.GetBlock<bhkNiCollisionObject>(id)) {
            std::cout << "{\"kind\":\"collision_object\",\"target\":"; ref(p->targetRef.index);
            std::cout << ",\"flags\":" << p->flags << ",\"body\":"; ref(p->bodyRef.index);
            std::cout << ",\"blend_gains\":";
            if (auto* blend = dynamic_cast<bhkBlendCollisionObject*>(p)) std::cout << '[' << bits(blend->heirGain) << ',' << bits(blend->velGain) << ']';
            else std::cout << "null"; std::cout << '}';
        } else if (auto* p = header.GetBlock<bhkRigidBody>(id)) body(*p, name == "bhkRigidBodyT");
        else if (auto* p = header.GetBlock<bhkBoxShape>(id)) {
            std::cout << "{\"kind\":\"box\",\"material\":" << p->GetMaterial() << ",\"radius\":" << bits(p->radius) << ",\"padding\":"; byte_span(raw + 8, 8);
            std::cout << ",\"half_extents\":"; vector_bits(p->dimensions, 3); std::cout << ",\"unused_w\":" << bits(p->radius2) << '}';
        } else if (auto* p = header.GetBlock<bhkSphereShape>(id)) std::cout << "{\"kind\":\"sphere\",\"material\":" << p->GetMaterial() << ",\"radius\":" << bits(p->radius) << '}';
        else if (auto* p = header.GetBlock<bhkCapsuleShape>(id)) {
            std::cout << "{\"kind\":\"capsule\",\"material\":" << p->GetMaterial() << ",\"radius\":" << bits(p->radius) << ",\"padding\":"; byte_span(raw + 8, 8);
            std::cout << ",\"first\":"; vector_bits(p->point1, 3); std::cout << ",\"first_radius\":" << bits(p->radius1) << ",\"second\":";
            vector_bits(p->point2, 3); std::cout << ",\"second_radius\":" << bits(p->radius2) << '}';
        } else if (auto* p = header.GetBlock<bhkConvexVerticesShape>(id)) {
            std::cout << "{\"kind\":\"convex_vertices\",\"material\":" << p->GetMaterial() << ",\"radius\":" << bits(p->radius) << ",\"vertex_property\":";
            property(p->vertsProp); std::cout << ",\"normal_property\":"; property(p->normalsProp);
            std::cout << ",\"vertices\":"; array(p->verts, [](const auto& v) { vector_bits(v, 4); });
            std::cout << ",\"planes\":"; array(p->normals, [](const auto& v) { vector_bits(v, 4); }); std::cout << '}';
        } else if (auto* p = header.GetBlock<bhkTransformShape>(id)) {
            std::cout << "{\"kind\":\"transform\",\"shape\":"; ref(p->shapeRef.index);
            std::cout << ",\"material\":" << p->material << ",\"radius\":" << bits(p->radius) << ",\"padding\":"; byte_span(raw + 12, 8);
            std::cout << ",\"matrix\":["; for (int row = 0; row < 4; ++row) { if (row) std::cout << ','; float v[4]; for (int col = 0; col < 4; ++col) v[col] = p->xform[row * 4 + col]; vector_bits(v, 4); }
            std::cout << "],\"convex_only\":" << (name == "bhkConvexTransformShape" ? "true" : "false") << '}';
        } else if (auto* p = header.GetBlock<bhkListShape>(id)) {
            std::cout << "{\"kind\":\"list\",\"shapes\":"; refs(p->subShapeRefs); std::cout << ",\"material\":" << p->GetMaterial() << ",\"shape_property\":";
            property(p->childShapeProp); std::cout << ",\"filter_property\":"; property(p->childFilterProp);
            std::cout << ",\"filters\":"; array(p->filters, [](const auto& f) { filter(f); }); std::cout << '}';
        } else if (auto* p = header.GetBlock<bhkMoppBvTreeShape>(id)) {
            std::cout << "{\"kind\":\"mopp\",\"shape\":"; ref(p->shapeRef.index);
            std::cout << ",\"unused\":[" << p->userData << ',' << p->shapeCollection << ',' << p->code << "],\"scale\":" << bits(p->scale) << ",\"offset\":";
            vector_bits(p->offset, 4); std::cout << ",\"code\":"; array(p->data, [](uint8_t b) { std::cout << unsigned(b); }); std::cout << '}';
        } else if (auto* p = header.GetBlock<bhkPackedNiTriStripsShape>(id)) {
            std::cout << "{\"kind\":\"packed_shape\",\"user_data\":" << p->userData << ",\"unused_01\":" << word(raw + 4)
                << ",\"radius\":" << bits(p->radius) << ",\"unused_02\":" << word(raw + 12) << ",\"scale\":"; vector_bits(p->scaling, 4);
            std::cout << ",\"radius_copy\":" << bits(p->radius2) << ",\"scale_copy\":"; vector_bits(p->scaling2, 4);
            std::cout << ",\"data\":"; ref(p->dataRef.index); std::cout << '}';
        } else if (auto* p = header.GetBlock<hkPackedNiTriStripsData>(id)) {
            if (p->compressed) throw std::runtime_error("pinned nifly cannot certify compressed packed vertices");
            std::cout << "{\"kind\":\"packed_data\",\"triangles\":";
            array(p->triData, [](const auto& t) { std::cout << "{\"indices\":"; scene_oracle::triangle(t.tri); std::cout << ",\"welding\":" << t.weldingInfo << '}'; });
            std::cout << ",\"vertex_count\":" << p->numVerts << ",\"compressed\":false,\"vertices\":";
            array(p->compressedVertData, [](const auto& v) { vector_bits(v, 3); });
            std::cout << ",\"compressed_words\":[],\"subparts\":";
            array(p->subPartData, [](const auto& s) { std::cout << "{\"filter\":"; filter(s.filter); std::cout << ",\"vertices\":" << s.numVerts << ",\"material\":" << s.material << '}'; });
            std::cout << '}';
        }
        std::cout << '}';
    }
    std::cout << "]}";
}
}
