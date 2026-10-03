// Original offline header projection. It identifies possible event-list owner
// kinds; it does not claim those forms have running scripts or captured values.
#include "../oracle-common/record_source.hpp"
using namespace fallout_records;

int wmain(int argc, wchar_t** argv) {
    try {
        if (argc != 3) throw std::runtime_error("usage: foreign-context-oracle Data_directory order_bundle");
        auto index = scan(argv[1], argv[2]);
        Hash hash;
        hash.update(Bytes{'F','R','C','O','N','T','E','X','T','1'});
        std::map<std::string, uint64_t> counts{{"forms",0},{"quests",0},{"placed",0},{"other",0},{"deleted",0}};
        for (const auto& winner : index.winners) {
            const auto& key = winner.first;
            const auto& header = winner.second.header;
            const std::string kind(header.begin(), header.begin()+4);
            const auto flags = static_cast<uint32_t>(integer(header,8,4));
            uint8_t category = 3;
            if (kind == "QUST") category = 1;
            else if (kind == "REFR" || kind == "ACHR" || kind == "ACRE" || kind == "PGRE" || kind == "PMIS" || kind == "PBEA") category = 2;
            Bytes row;
            text(row,key.origin);
            append(row,key.local,4);
            row.insert(row.end(),header.begin(),header.begin()+4);
            append(row,flags,4);
            row.push_back(category);
            hash.update(row);
            ++counts["forms"];
            if (flags & 0x20) ++counts["deleted"];
            ++counts[category == 1 ? "quests" : category == 2 ? "placed" : "other"];
        }
        Object count_json;
        for (const auto& count : counts) count_json[count.first] = std::to_string(count.second);
        const auto projection = object({{"counts",object(count_json)},
            {"classification_sha256",quote(hash.finish())},
            {"winning_headers_sha256",quote(index.winners_sha256)}});
        std::cout << object({{"schema_version","1"},{"profile",quote("nv-original")},
            {"sources",index.sources_json()},{"metadata",index.metadata_json()},
            {"context_content",projection},{"live_values_captured","false"},
            {"retail_parity_accepted","false"}}) << '\n';
        return 0;
    } catch (const std::exception& error) {
        std::cerr << "foreign-context-oracle: " << error.what() << '\n';
        return 1;
    }
}
