// Completeness is checked against the earlier, bound full source-table bundle.
// Winner selection is rebuilt from original headers rather than from the new
// catalogue's chosen records, so accidentally omitted units change this digest.
#pragma once
static std::string coverage(const HeaderIndex& index, const Bytes& bundle) {
    size_t cursor = 8; uint64_t full_units = 0, full_records = 0, winning_records = 0;
    std::set<std::pair<size_t, uint64_t>> seen;
    std::map<std::pair<Key, size_t>, Bytes> winning;
    for (; cursor < bundle.size();) {
        const auto source_index = static_cast<size_t>(integer(bundle, cursor, 1)); ++cursor;
        const auto name = take(bundle, cursor, 4); const std::string kind(name.begin(), name.end());
        const auto raw = static_cast<uint32_t>(integer(bundle, cursor, 4)); cursor += 4;
        const auto offset = integer(bundle, cursor, 8); cursor += 8;
        const auto length = static_cast<size_t>(integer(bundle, cursor, 4)); cursor += 4;
        if (source_index >= index.plugins.size() || length > 64 * 1024 * 1024 || !seen.emplace(source_index, offset).second
            || ++full_records > 1000000) throw std::runtime_error("coverage source/record budget");
        const auto& plugin = index.plugins[source_index]; const auto header = plugin.source->read(offset, 24);
        if (std::string(header.begin(), header.begin() + 4) != kind || integer(header, 12, 4) != raw) throw std::runtime_error("coverage source identity differs");
        const auto flags = integer(header, 8, 4), stored = integer(header, 4, 4); const auto payload = take(bundle, cursor, length);
        if (!(flags & 0x40000)) {
            if (stored != length || plugin.source->read(offset + 24, length) != payload) throw std::runtime_error("coverage uncompressed body differs");
        } else if (stored < 4 || integer(plugin.source->read(offset + 24, 4), 0, 4) != length) throw std::runtime_error("coverage decoded extent differs");
        const auto key = resolve(plugin.name, plugin.masters, raw); if (!key) throw std::runtime_error("coverage null identity");
        const auto winner = index.winners.find(*key); if (winner == index.winners.end()) throw std::runtime_error("coverage definition absent");
        const bool live_winner = winner->second.source == source_index && winner->second.offset == offset && !(flags & 0x20);
        const auto units = fallout_tables::units(payload); if (units.empty()) throw std::runtime_error("coverage has no script unit");
        winning_records += live_winner;
        for (const auto& unit : units) {
            if (++full_units > 262144) throw std::runtime_error("coverage unit budget");
            if (!live_winner) continue;
            Bytes metadata;
            for (const auto& field : unit) {
                metadata.insert(metadata.end(), field.kind.begin(), field.kind.end()); append(metadata, field.offset, 4);
                append(metadata, field.data.size(), 4); metadata.insert(metadata.end(), field.data.begin(), field.data.end());
            }
            Bytes tuple; version_text(tuple, key->origin); append(tuple, key->local, 4); version_text(tuple, plugin.name);
            append(tuple, offset, 8); append(tuple, unit.front().offset, 4); version_text(tuple, fallout_tables::hash(metadata));
            if (!winning.emplace(std::make_pair(*key, unit.front().offset), std::move(tuple)).second) throw std::runtime_error("duplicate coverage unit");
        }
    }
    Hash digest; for (const auto& item : winning) digest.update(item.second);
    return object({{"full_source_records", std::to_string(full_records)}, {"full_source_units", std::to_string(full_units)},
        {"winning_records", std::to_string(winning_records)}, {"winning_units", std::to_string(winning.size())},
        {"excluded_units", std::to_string(full_units - winning.size())}, {"winning_metadata_sha256", quote(digest.finish())}});
}
