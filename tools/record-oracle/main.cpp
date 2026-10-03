// Original direct source-header and dialogue membership comparison.
#include "../oracle-common/record_source.hpp"
using namespace fallout_records;

int wmain(int argc, wchar_t** argv) {
    try {
        if (argc != 3) throw std::runtime_error("usage: record-oracle Data_directory order_bundle");
        auto index = scan(argv[1], argv[2]);
        const auto& plugins = index.plugins; const auto& winners = index.winners;
        std::vector<std::string> rows, topics; std::map<Key, std::vector<Key>> by_topic;
        std::map<std::string, uint64_t> counts;
        for (const auto name : {"winning_infos", "linked_infos", "deleted_infos", "missing_parents", "null_parents", "missing_topics", "deleted_topics", "wrong_topic_kinds", "topics_with_linked_infos"}) counts[name] = 0;
        for (const auto& winner : winners) {
            const auto& key = winner.first; const auto& entry = winner.second; const auto& source = plugins[entry.source];
            const std::string kind(entry.header.begin(), entry.header.begin() + 4); if (kind != "INFO") continue;
            ++counts["winning_infos"]; const auto flags = static_cast<uint32_t>(integer(entry.header, 8, 4));
            const auto topic_key = entry.topic ? resolve(source.name, source.masters, *entry.topic) : std::nullopt;
            const auto target = topic_key ? winners.find(*topic_key) : winners.end(); std::string status, topic = "null";
            if (target != winners.end()) {
                const auto& entry = target->second;
                topic = object({{"source_plugin", quote(plugins[entry.source].name)}, {"record_file_offset", std::to_string(entry.offset)},
                    {"record_flags", std::to_string(integer(entry.header, 8, 4))}, {"record_kind", quote(std::string(entry.header.begin(), entry.header.begin() + 4))}});
            }
            if (flags & 0x20) { status = "deleted_info"; ++counts["deleted_infos"]; }
            else if (!entry.topic) { status = "missing_parent"; ++counts["missing_parents"]; }
            else if (!topic_key) { status = "null_parent"; ++counts["null_parents"]; }
            else if (target == winners.end()) { status = "missing_topic"; ++counts["missing_topics"]; }
            else if (integer(target->second.header, 8, 4) & 0x20) { status = "deleted_topic"; ++counts["deleted_topics"]; }
            else if (std::string(target->second.header.begin(), target->second.header.begin() + 4) != "DIAL") { status = "wrong_topic_kind"; ++counts["wrong_topic_kinds"]; }
            else { status = "linked"; ++counts["linked_infos"]; by_topic[*topic_key].push_back(key); }
            rows.push_back(object({{"info_key", key.json()}, {"source_plugin", quote(source.name)}, {"record_file_offset", std::to_string(entry.offset)},
                {"record_flags", std::to_string(flags)}, {"raw_parent_topic", entry.topic ? std::to_string(*entry.topic) : "null"},
                {"topic_key", topic_key ? topic_key->json() : "null"}, {"topic", topic}, {"status", quote(status)}}));
        }
        counts["topics_with_linked_infos"] = by_topic.size();
        for (const auto& topic : by_topic) { std::vector<std::string> infos; for (const auto& info : topic.second) infos.push_back(info.json()); topics.push_back(object({{"topic_key", topic.first.json()}, {"info_keys", array(infos)}})); }
        Object count_json; for (const auto& count : counts) count_json[count.first] = std::to_string(count.second);
        std::cout << object({{"schema_version", "1"}, {"profile", quote("nv-original")}, {"plugins", index.sources_json()},
            {"metadata", index.metadata_json()},
            {"membership", object({{"counts", object(count_json)}, {"rows", array(rows)}, {"topics", array(topics)},
                {"payloads_validated", "false"}, {"retail_selection_order_verified", "false"}})}}) << '\n';
        return 0;
    } catch (const std::exception& error) { std::cerr << "record-oracle: " << error.what() << '\n'; return 1; }
}
