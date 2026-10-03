// Owned source-structure oracle. Validation is a bounded forward audit; pairing
// scans backward from closing delimiters, unlike Rust's forward open-group plan.
// Neither path chooses a VM successor or evaluates a condition.
#include "../oracle-common/expression_source.hpp"

struct Instruction {
    size_t start, operand, end;
    uint16_t opcode;
    bool caller;
};
struct Arm {
    size_t instruction, enclosing_if, next_delimiter, end_if, depth;
};
struct Link {
    size_t source, target, distance;
    uint32_t raw;
    bool event;
};
struct Event {
    size_t begin, end;
    uint16_t id;
};
struct Finding : std::runtime_error {
    std::string json;
    Finding(const std::string &kind, size_t offset, std::optional<uint16_t> opcode = {},
            std::optional<uint32_t> raw = {}, std::optional<size_t> distance = {})
        : std::runtime_error(kind),
          json(object({{"kind", quote_json(kind)},
                       {"instruction_scda_offset", std::to_string(offset)},
                       {"opcode", opcode ? std::to_string(*opcode) : "null"},
                       {"raw_word", raw ? std::to_string(*raw) : "null"},
                       {"observed_distance", distance ? std::to_string(*distance) : "null"}})) {}
};
struct Totals {
    size_t instructions = 0, complete_bodies = 0, structural_issues = 0;
    size_t events = 0, arms = 0, links = 0, maximum_depth = 0;
};
static std::vector<Instruction> frame(const Bytes &bytes) {
    std::vector<Instruction> rows;
    for (size_t at = 0; at < bytes.size();) {
        if (rows.size() >= 262144)
            throw std::runtime_error("instruction budget");
        const size_t start = at;
        auto opcode = word(bytes, at);
        const bool caller = opcode == 0x1c;
        if (caller) {
            (void)word(bytes, at);
            opcode = word(bytes, at);
        }
        const auto length = word(bytes, at);
        const size_t operand = at;
        if (length > bytes.size() - at)
            throw std::runtime_error("truncated instruction");
        at += length;
        if (opcode == 0x10 && length < 6)
            throw std::runtime_error("short event header");
        rows.push_back({start, operand, at, opcode, caller});
    }
    return rows;
}
static size_t audit(const Bytes &bytes, const std::vector<Instruction> &rows) {
    bool event = false;
    size_t depth = 0, maximum_depth = 0;
    std::vector<bool> has_else;
    for (const auto &row : rows) {
        const auto opcode = row.opcode;
        const auto size = row.end - row.operand;
        auto fail = [&](const std::string &kind) { throw Finding(kind, row.start, opcode); };
        if (opcode < 0x1000 && row.caller)
            fail("statement_reference_prefix");
        if (opcode == 0x10) {
            if (event || depth)
                fail("nested_event");
            event = true;
        } else if (opcode == 0x11) {
            if (size)
                fail("statement_operands");
            if (!event)
                fail("orphan_end");
            if (depth)
                fail("conditional_crosses_event");
            event = false;
        } else if (opcode == 0x16 || opcode == 0x17 || opcode == 0x18) {
            if (opcode == 0x17) {
                if (size != 2)
                    fail("statement_operands");
            } else if (size < 4 || number(bytes, row.operand + 2, 2) != size - 4)
                fail("conditional_expression_extent");
            if (opcode == 0x16) {
                if (depth >= 65536)
                    fail("depth_budget");
                ++depth;
                maximum_depth = std::max(maximum_depth, depth);
                has_else.push_back(false);
            } else {
                if (!depth)
                    fail("orphan_arm");
                if (has_else.back())
                    fail("arm_after_else");
                has_else.back() = opcode == 0x17;
            }
        } else if (opcode == 0x19) {
            if (size)
                fail("statement_operands");
            if (!depth)
                fail("orphan_end_if");
            --depth;
            has_else.pop_back();
        } else if (opcode == 0x1d || opcode == 0x1e) {
            if (size)
                fail("statement_operands");
        } else if (!((opcode >= 0x12 && opcode <= 0x15) || opcode == 0x1f || opcode >= 0x1000))
            fail("unsupported_opcode");
    }
    if (event || depth)
        throw Finding(event ? "unclosed_event" : "unclosed_conditional", bytes.size());
    return maximum_depth;
}
static std::string structure(const Bytes &bytes, const std::vector<Instruction> &rows,
                             Totals &totals) {
    const auto maximum_depth = audit(bytes, rows);
    std::vector<std::optional<Arm>> arms(rows.size());
    std::vector<std::optional<Link>> links(rows.size());
    std::vector<std::optional<Event>> events(rows.size());
    struct Close {
        size_t end, next;
        std::vector<size_t> arms;
    };
    std::vector<Close> closing;
    std::optional<size_t> end_event;
    for (size_t remaining = rows.size(); remaining; --remaining) {
        const size_t index = remaining - 1;
        const auto &row = rows[index];
        if (row.opcode == 0x11)
            end_event = index;
        else if (row.opcode == 0x10) {
            const auto end = *end_event;
            events[index] = Event{index, end, static_cast<uint16_t>(number(bytes, row.operand, 2))};
            links[index] =
                Link{index, end, rows[end].end - row.end, number(bytes, row.operand + 2, 4), true};
            end_event.reset();
        } else if (row.opcode == 0x19)
            closing.push_back({index, index, {}});
        else if (row.opcode == 0x16 || row.opcode == 0x17 || row.opcode == 0x18) {
            auto &close = closing.back();
            arms[index] = Arm{index, SIZE_MAX, close.next, close.end, closing.size()};
            links[index] = Link{index, close.next, close.next - index - 1,
                                number(bytes, row.operand, 2), false};
            close.arms.push_back(index);
            close.next = index;
            if (row.opcode == 0x16) {
                for (const auto arm : close.arms)
                    arms[arm]->enclosing_if = index;
                closing.pop_back();
            }
        }
    }
    std::vector<std::string> arm_rows, event_rows, link_rows;
    for (size_t i = 0; i < rows.size(); ++i) {
        if (links[i]) {
            const auto &link = *links[i];
            if (link.raw != link.distance)
                throw Finding("raw_distance_mismatch", rows[i].start, rows[i].opcode, link.raw,
                              link.distance);
            link_rows.push_back(object(
                {{"source_instruction", std::to_string(link.source)},
                 {"delimiter_instruction", std::to_string(link.target)},
                 {"raw_word", std::to_string(link.raw)},
                 {"observed_distance", std::to_string(link.distance)},
                 {"relation",
                  quote_json(link.event ? "event_byte_span" : "intervening_instructions")}}));
        }
        if (arms[i]) {
            const auto &arm = *arms[i];
            arm_rows.push_back(object({{"instruction", std::to_string(arm.instruction)},
                                       {"enclosing_if", std::to_string(arm.enclosing_if)},
                                       {"next_delimiter", std::to_string(arm.next_delimiter)},
                                       {"end_if", std::to_string(arm.end_if)},
                                       {"depth", std::to_string(arm.depth)}}));
        }
        if (events[i]) {
            const auto &event = *events[i];
            event_rows.push_back(object({{"begin_instruction", std::to_string(event.begin)},
                                         {"end_instruction", std::to_string(event.end)},
                                         {"event_id", std::to_string(event.id)}}));
        }
    }
    ++totals.complete_bodies;
    totals.events += event_rows.size();
    totals.arms += arm_rows.size();
    totals.links += link_rows.size();
    totals.maximum_depth = std::max(totals.maximum_depth, maximum_depth);
    return object({{"events", array(event_rows)},
                   {"arms", array(arm_rows)},
                   {"links", array(link_rows)},
                   {"maximum_depth", std::to_string(maximum_depth)}});
}
static int compare(const std::filesystem::path &path, bool diagnostic) {
    // Keep the source handle open with no write sharing for the whole comparison.
    const HANDLE handle = CreateFileW(path.c_str(), GENERIC_READ, FILE_SHARE_READ, nullptr,
                                      OPEN_EXISTING, FILE_ATTRIBUTE_NORMAL, nullptr);
    if (handle == INVALID_HANDLE_VALUE)
        throw std::runtime_error("bundle open");
    struct Guard {
        HANDLE handle;
        ~Guard() { CloseHandle(handle); }
    } guard{handle};
    LARGE_INTEGER length{};
    if (!GetFileSizeEx(handle, &length) || length.QuadPart < 8 ||
        length.QuadPart > 66 * 1024 * 1024)
        throw std::runtime_error("bundle byte budget");
    Bytes bundle(static_cast<size_t>(length.QuadPart));
    DWORD read = 0;
    if (!ReadFile(handle, bundle.data(), static_cast<DWORD>(bundle.size()), &read, nullptr) ||
        read != bundle.size() || std::string(bundle.begin(), bundle.begin() + 8) != "FROBS001")
        throw std::runtime_error("bundle read or magic");
    Totals totals;
    std::vector<std::string> bodies;
    for (size_t at = 8; at < bundle.size();) {
        if (bodies.size() >= 65536)
            throw std::runtime_error("body budget");
        const auto length = number(bundle, at, 4);
        at += 4;
        if (length > 4 * 1024 * 1024)
            throw std::runtime_error("body byte budget");
        const auto bytes = take(bundle, at, length);
        const auto rows = frame(bytes);
        if (rows.size() > 2000000 - totals.instructions)
            throw std::runtime_error("aggregate instruction budget");
        totals.instructions += rows.size();
        std::string plan = "null", issue = "null";
        try {
            plan = structure(bytes, rows, totals);
        } catch (const Finding &finding) {
            if (!diagnostic)
                throw;
            issue = finding.json;
            ++totals.structural_issues;
        }
        bodies.push_back(object({{"bytes", std::to_string(bytes.size())},
                                 {"sha256", quote_json(digest(bytes))},
                                 {"instructions", std::to_string(rows.size())},
                                 {"structure", plan},
                                 {"issue", issue}}));
    }
    const auto counts = object({{"instructions", std::to_string(totals.instructions)},
                                {"complete_bodies", std::to_string(totals.complete_bodies)},
                                {"structural_issues", std::to_string(totals.structural_issues)},
                                {"events", std::to_string(totals.events)},
                                {"arms", std::to_string(totals.arms)},
                                {"links", std::to_string(totals.links)},
                                {"maximum_depth", std::to_string(totals.maximum_depth)}});
    std::cout << object({{"schema_version", "1"},
                         {"structure", object({{"schema_version", "1"},
                                               {"bundle_sha256", quote_json(digest(bundle))},
                                               {"compiled_bodies", std::to_string(bodies.size())},
                                               {"counts", counts},
                                               {"bodies", array(bodies)},
                                               {"execution_ready", "false"}})},
                         {"execution_ready", "false"}})
              << '\n';
    return totals.structural_issues ? 1 : 0;
}
int wmain(int argc, wchar_t **argv) {
    try {
        if (argc != 2 && argc != 3)
            throw std::runtime_error(
                "usage: control-flow-oracle comparison-bundle [--diagnose-structure]");
        const bool diagnostic = argc == 3 && std::wstring(argv[2]) == L"--diagnose-structure";
        if (argc == 3 && !diagnostic)
            throw std::runtime_error("unknown option");
        return compare(argv[1], diagnostic);
    } catch (const std::exception &error) {
        std::cerr << "control-flow-oracle: " << error.what() << '\n';
        return 1;
    }
}
