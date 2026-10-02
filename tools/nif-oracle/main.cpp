// SPDX-License-Identifier: GPL-3.0-only
// Independent offline oracle. This executable is never linked into fallout.exe.
#include "NifFile.hpp"
#include "scene.hpp"
#include "collision.hpp"
#include <algorithm>
#include <filesystem>
#include <iomanip>
#include <iostream>
#include <vector>

int main(int argc, char** argv) {
    if (argc != 2 && !(argc == 3 && (std::string(argv[2]) == "--scene" || std::string(argv[2]) == "--scene-diagnostics" || std::string(argv[2]) == "--collision"))) {
        std::cerr << "usage: nif-oracle CACHE_DIRECTORY_OR_FILE [--scene|--scene-diagnostics|--collision]\n";
        return 2;
    }
    try {
        const std::filesystem::path input(argv[1]);
        std::vector<std::filesystem::path> paths;
        if (std::filesystem::is_directory(input)) {
            for (const auto& entry : std::filesystem::directory_iterator(input)) {
                if (entry.is_regular_file() && entry.path().extension() == ".blob")
                    paths.push_back(entry.path());
            }
        } else {
            paths.push_back(input);
        }
        std::sort(paths.begin(), paths.end());
        bool failed = paths.empty();
        const bool collision_mode = argc == 3 && std::string(argv[2]) == "--collision";
        if (collision_mode) std::cout << "{\"float_encoding\":\"ieee754-binary32-bits\",\"oracle_binary_sha256\":"
            << std::quoted(collision_oracle::file_sha256(argv[0])) << ",\"files\":[";
        else std::cout << '[';
        for (size_t file_index = 0; file_index < paths.size(); ++file_index) {
            if (file_index) std::cout << ',';
            if (argc == 3 && std::string(argv[2]) == "--collision") {
                collision_oracle::write(paths[file_index]);
                continue;
            }
            nifly::NifFile file;
            const int result = file.Load(paths[file_index]);
            std::cout << "{\"file\":" << std::quoted(paths[file_index].filename().string())
                      << ",\"load_code\":" << result;
            if (result == 0) {
                const auto& header = file.GetHeader();
                const auto& version = header.GetVersion();
                std::cout << ",\"version\":" << static_cast<uint32_t>(version.File())
                          << ",\"user_version\":" << version.User()
                          << ",\"bethesda_version\":" << version.Stream()
                          << ",\"blocks\":[";
                for (uint32_t i = 0; i < header.GetNumBlocks(); ++i) {
                    if (i) std::cout << ',';
                    std::cout << "{\"type\":" << std::quoted(header.GetBlockTypeStringById(i))
                              << ",\"type_index\":" << header.GetBlockTypeIndex(i)
                              << ",\"bytes\":" << header.GetBlockSize(i) << '}';
                }
                std::cout << "],\"roots\":[";
                const auto& roots = header.GetRootBlockIds();
                for (size_t i = 0; i < roots.size(); ++i) {
                    if (i) std::cout << ',';
                    if (roots[i] == UINT32_MAX) std::cout << "null";
                    else std::cout << roots[i];
                }
                std::cout << ']';
                if (argc == 3) scene_oracle::write(paths[file_index], std::string(argv[2]) == "--scene-diagnostics");
            } else {
                failed = true;
            }
            std::cout << '}';
        }
        std::cout << (collision_mode ? "]}\n" : "]\n");
        return failed ? 1 : 0;
    } catch (const std::exception& error) {
        std::cerr << "nif-oracle: " << error.what() << '\n';
        return 1;
    }
}
