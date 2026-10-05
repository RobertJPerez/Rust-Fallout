// GPL-3.0-only independent validation tool; never linked into the Rust runtime.
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Mutagen.Bethesda.Skyrim;
using Mutagen.Bethesda.Plugins;
using Mutagen.Bethesda.Plugins.Records;

if (args.Length == 1 && args[0] == "--shape")
{
    var assembly = typeof(ISkyrimModGetter).Assembly;
    foreach (string name in new[] { "INpcGetter", "INpcSpawnGetter", "INpcConfigurationGetter", "ILeveledSpellGetter", "ILeveledNpcGetter", "ILeveledItemGetter", "ILeveledSpellEntryGetter", "ILeveledNpcEntryGetter", "ILeveledItemEntryGetter", "ILeveledSpellEntryDataGetter", "ILeveledNpcEntryDataGetter", "ILeveledItemEntryDataGetter" })
    {
        var type = assembly.GetType("Mutagen.Bethesda.Skyrim." + name);
        if (type is null) continue;
        Console.WriteLine(type.Name);
        foreach (var parent in type.GetInterfaces()) Console.WriteLine($"  inherits {parent.Name}");
        foreach (var property in type.GetProperties().OrderBy(p => p.Name))
            Console.WriteLine($"  {property.Name}: {property.PropertyType}");
    }
    return 0;
}
if (args.Length != 1) throw new ArgumentException("usage: trace-oracle TRACE_JSON | --shape");
Encoding.RegisterProvider(CodePagesEncodingProvider.Instance);
var encoding = Encoding.GetEncoding(1252);
var report = JsonNode.Parse(File.ReadAllText(args[0]))!;
if (report["schema_version"]!.GetValue<int>() != 2) throw new InvalidDataException("unsupported trace schema");
var actorPaths = report["actor_paths"]!;
var contexts = report["attachments"]!.AsArray().Select(a => a!["record"]!)
    .Concat(report["definition_candidates"]!.AsArray().Select(a => a!))
    .Concat(report["direct_references"]!.AsArray().Select(a => a!["record"]!))
    .Concat(actorPaths["missing_script_effects"]!.AsArray().Select(a => a!))
    .Concat(actorPaths["spell_candidates"]!.AsArray().Select(a => a!))
    .Concat(actorPaths["actor_definitions"]!.AsArray().Select(a => a!))
    .Concat(actorPaths["links"]!.AsArray().Select(a => a!["record"]!))
    .Concat(actorPaths["unresolved_links"]!.AsArray().Select(a => a!["record"]!))
    .Concat(report["containing_cell_candidates"]!.AsArray().Select(a => a!))
    .Concat(report["containing_world_candidates"]!.AsArray().Select(a => a!)).ToArray();
string Text(JsonNode? bytes) => encoding.GetString(bytes!.AsArray().Select(b => b!.GetValue<byte>()).ToArray());
string Key(JsonNode node) => node["origin_plugin"]!.GetValue<string>().ToLowerInvariant() + ":" + node["local_id"]!.GetValue<uint>().ToString("X6");
string Form(FormKey key) => key.ModKey.FileName.String.ToLowerInvariant() + ":" + key.ID.ToString("X6");
var targets = report["attachments"]!.AsArray().Where(a => a!["record"]!["source_key"] != null).Select(a => Key(a!["record"]!["source_key"]!)).ToHashSet();
long comparisons = 0, mismatches = 0, distinctRecords = 0;
var examples = new List<object>();
void Check(string path, object? rust, object? actual)
{
    comparisons++;
    if (!JsonNode.DeepEquals(JsonSerializer.SerializeToNode(rust), JsonSerializer.SerializeToNode(actual)))
    {
        mismatches++;
        if (examples.Count < 64) examples.Add(new { path, rust, independent = actual });
    }
}
string EdgeKey(string file, uint record, string field, uint target) => $"{file.ToLowerInvariant()}:{record:X8}:{field}:{target:X8}";
string RecordKey(string file, uint record) => $"{file.ToLowerInvariant()}:{record:X8}";
var expectedEdges = report["direct_references"]!.AsArray().Select(e => EdgeKey(
    e!["record"]!["source_plugin"]!.GetValue<string>(), e["record"]!["header"]!["form_id"]!.GetValue<uint>(),
    e["field"]!.GetValue<string>(), e["target_raw"]!.GetValue<uint>())).GroupBy(e => e).ToDictionary(g => g.Key, g => g.Count());
var actualEdges = new Dictionary<string, int>();
var actorTargets = actorPaths["spell_candidates"]!.AsArray()
    .Concat(actorPaths["actor_definitions"]!.AsArray())
    .Where(c => c!["source_key"] != null)
    .Select(c => Key(c!["source_key"]!)).ToHashSet();
var expectedActorEdges = actorPaths["links"]!.AsArray().Select(e => EdgeKey(
    e!["record"]!["source_plugin"]!.GetValue<string>(), e["record"]!["header"]!["form_id"]!.GetValue<uint>(),
    e["field"]!.GetValue<string>(), e["target_raw"]!.GetValue<uint>())).GroupBy(e => e).ToDictionary(g => g.Key, g => g.Count());
var actualActorEdges = new Dictionary<string, int>();
var expectedActorLayouts = actorPaths["links"]!.AsArray().Select(e =>
    EdgeKey(e!["record"]!["source_plugin"]!.GetValue<string>(), e["record"]!["header"]!["form_id"]!.GetValue<uint>(),
        e["field"]!.GetValue<string>(), e["target_raw"]!.GetValue<uint>()) + ":" +
    Convert.ToHexString(e["field_bytes"]!.AsArray().Select(b => b!.GetValue<byte>()).ToArray()))
    .GroupBy(e => e).ToDictionary(g => g.Key, g => g.Count());
var actualActorLayouts = new Dictionary<string, int>();
var expectedActorDefinitions = actorPaths["actor_definitions"]!.AsArray()
    .Select(c => RecordKey(c!["source_plugin"]!.GetValue<string>(), c["header"]!["form_id"]!.GetValue<uint>()))
    .ToHashSet();
var actualActorDefinitions = new HashSet<string>();
var sources = new List<object>();
foreach (var source in report["sources"]!.AsArray())
{
    string file = source!["file"]!.GetValue<string>();
    Console.Error.WriteLine("Independently tracing " + file);
    if (file != Path.GetFileName(file) || file.Contains(':')) throw new InvalidDataException("unsafe source filename");
    string path = Path.Combine(report["data"]!.GetValue<string>(), file);
    using var input = new FileStream(path, FileMode.Open, FileAccess.Read, FileShare.Read);
    string digest = Convert.ToHexStringLower(SHA256.HashData(input));
    if (digest != source["sha256"]!.GetValue<string>() || input.Length != source["bytes"]!.GetValue<long>()) throw new InvalidDataException("source hash mismatch");
    input.Position = 0;
    var modKey = ModKey.FromFileName(file);
    using var mod = SkyrimMod.CreateFromBinaryOverlay(input, SkyrimRelease.SkyrimSE, modKey);
    var masters = mod.ModHeader.MasterReferences.Select(m => m.Master).Append(modKey).ToArray();
    uint Raw(FormKey key)
    {
        if (key.IsNull) return 0;
        int index = Array.IndexOf(masters, key.ModKey);
        if (index < 0 || index > 255) throw new InvalidDataException("unrepresentable independent FormKey");
        return (uint)index << 24 | key.ID;
    }
    void Edge(IMajorRecordGetter record, string field, FormKey target)
    {
        if (target.IsNull || !targets.Contains(Form(target))) return;
        string key = EdgeKey(file, Raw(record.FormKey), field, Raw(target));
        actualEdges[key] = actualEdges.GetValueOrDefault(key) + 1;
    }
    void ActorEdge(IMajorRecordGetter record, string field, FormKey target, byte[] rawField)
    {
        if (target.IsNull || !actorTargets.Contains(Form(target))) return;
        string key = EdgeKey(file, Raw(record.FormKey), field, Raw(target));
        actualActorEdges[key] = actualActorEdges.GetValueOrDefault(key) + 1;
        string layout = key + ":" + Convert.ToHexString(rawField);
        actualActorLayouts[layout] = actualActorLayouts.GetValueOrDefault(layout) + 1;
    }
    byte[] LeveledBytes(short level, short unknown, FormKey target, short count, short unknown2)
    {
        var bytes = new byte[12];
        System.Buffers.Binary.BinaryPrimitives.WriteInt16LittleEndian(bytes.AsSpan(0, 2), level);
        System.Buffers.Binary.BinaryPrimitives.WriteInt16LittleEndian(bytes.AsSpan(2, 2), unknown);
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32LittleEndian(bytes.AsSpan(4, 4), Raw(target));
        System.Buffers.Binary.BinaryPrimitives.WriteInt16LittleEndian(bytes.AsSpan(8, 2), count);
        System.Buffers.Binary.BinaryPrimitives.WriteInt16LittleEndian(bytes.AsSpan(10, 2), unknown2);
        return bytes;
    }
    byte[] FormBytes(uint raw)
    {
        var bytes = new byte[4];
        System.Buffers.Binary.BinaryPrimitives.WriteUInt32LittleEndian(bytes, raw);
        return bytes;
    }
    var requested = contexts.Where(c => c["source_plugin"]!.GetValue<string>() == file).GroupBy(c => c["header"]!["form_id"]!.GetValue<uint>()).ToDictionary(g => g.Key, g => g.ToArray());
    var seen = new HashSet<uint>();
    var parentCells = new Dictionary<uint, uint>();
    foreach (var record in mod.EnumerateMajorRecords())
    {
        if (record is ICellGetter cell)
            foreach (var placed in cell.Persistent.Concat(cell.Temporary))
                if (requested.ContainsKey(Raw(placed.FormKey))) parentCells.Add(Raw(placed.FormKey), Raw(cell.FormKey));
        uint id = Raw(record.FormKey);
        if (requested.TryGetValue(id, out var rows))
        {
            seen.Add(id);
            distinctRecords++;
            foreach (var row in rows)
            {
                string context = $"{file}:{id:X8}";
                Check(context + ".source_key", row["source_key"] == null ? null : Key(row["source_key"]!), Form(record.FormKey));
                Check(context + ".editor_id", row["editor_id"] == null ? null : Text(row["editor_id"]), record.EditorID);
                Check(context + ".flags", row["header"]!["flags"]!.GetValue<uint>(), unchecked((uint)record.MajorRecordFlagsRaw));
                Check(context + ".deleted", row["deleted"]!.GetValue<bool>(), record.IsDeleted);
                Check(context + ".vmad_present", row["vmad_present"]!.GetValue<bool>(), record is IHaveVirtualMachineAdapterGetter { VirtualMachineAdapter: not null });
                if (row["initially_disabled"] != null) Check(context + ".initially_disabled", row["initially_disabled"]!.GetValue<bool>(), (record.MajorRecordFlagsRaw & 0x800) != 0);
                if (row["cell_flags_raw"] != null)
                {
                    var bytes = row["cell_flags_raw"]!.AsArray().Select(b => b!.GetValue<byte>()).ToArray();
                    uint flags = bytes.Length == 1 ? bytes[0] : BitConverter.ToUInt16(bytes);
                    Check(context + ".cell_flags", flags, record is ICellGetter c ? (uint)c.Flags : uint.MaxValue);
                }
            }
        }
        if (actorTargets.Contains(Form(record.FormKey)) &&
            record is INpcGetter or ILeveledSpellGetter or ILeveledNpcGetter or IPlacedNpcGetter)
            actualActorDefinitions.Add(RecordKey(file, id));
        switch (record)
        {
            case IPlacedNpcGetter r:
                Edge(r, "NAME", r.Base.FormKey);
                ActorEdge(r, "NAME", r.Base.FormKey, FormBytes(Raw(r.Base.FormKey)));
                if (r.EnableParent != null) Edge(r, "XESP", r.EnableParent.Reference.FormKey);
                break;
            case IPlacedObjectGetter r:
                Edge(r, "NAME", r.Base.FormKey);
                if (r.EnableParent != null) Edge(r, "XESP", r.EnableParent.Reference.FormKey);
                break;
            case ISpellGetter r: foreach (var effect in r.Effects) Edge(r, "EFID", effect.BaseEffect.FormKey); break;
            case IScrollGetter r: foreach (var effect in r.Effects) Edge(r, "EFID", effect.BaseEffect.FormKey); break;
            case IIngestibleGetter r: foreach (var effect in r.Effects) Edge(r, "EFID", effect.BaseEffect.FormKey); break;
            case IIngredientGetter r: foreach (var effect in r.Effects) Edge(r, "EFID", effect.BaseEffect.FormKey); break;
            case IObjectEffectGetter r: foreach (var effect in r.Effects) Edge(r, "EFID", effect.BaseEffect.FormKey); break;
        }
        switch (record)
        {
            case INpcGetter r:
                if (r.ActorEffect is not null)
                    foreach (var spell in r.ActorEffect)
                        ActorEdge(r, "SPLO", spell.FormKey, FormBytes(Raw(spell.FormKey)));
                if (!r.Template.IsNull)
                    ActorEdge(r, "TPLT", r.Template.FormKey, FormBytes(Raw(r.Template.FormKey)));
                break;
            case ILeveledSpellGetter r:
                if (r.Entries is not null)
                    foreach (var entry in r.Entries)
                        if (entry.Data is not null)
                            ActorEdge(r, "LVLO", entry.Data.Reference.FormKey,
                                LeveledBytes(entry.Data.Level, entry.Data.Unknown, entry.Data.Reference.FormKey,
                                    entry.Data.Count, entry.Data.Unknown2));
                break;
            case ILeveledNpcGetter r:
                if (r.Entries is not null)
                    foreach (var entry in r.Entries)
                        if (entry.Data is not null)
                            ActorEdge(r, "LVLO", entry.Data.Reference.FormKey,
                                LeveledBytes(entry.Data.Level, entry.Data.Unknown, entry.Data.Reference.FormKey,
                                    entry.Data.Count, entry.Data.Unknown2));
                break;
        }
    }
    foreach (uint id in requested.Keys.Except(seen)) Check($"{file}:{id:X8}.present", true, false);
    foreach (var pair in requested)
        foreach (var row in pair.Value)
            if (row["initially_disabled"] != null)
                Check($"{file}:{pair.Key:X8}.cell", row["containing_cell_raw"]?.GetValue<uint>(), parentCells.TryGetValue(pair.Key, out uint cell) ? cell : (uint?)null);
    sources.Add(new { file, sha256 = digest, requested_records = requested.Count, found_records = seen.Count });
}
foreach (string key in expectedEdges.Keys.Union(actualEdges.Keys)) Check("edge." + key, expectedEdges.GetValueOrDefault(key), actualEdges.GetValueOrDefault(key));
foreach (string key in expectedActorEdges.Keys.Union(actualActorEdges.Keys))
    Check("actor_edge." + key, expectedActorEdges.GetValueOrDefault(key), actualActorEdges.GetValueOrDefault(key));
foreach (string key in expectedActorLayouts.Keys.Union(actualActorLayouts.Keys))
    Check("actor_field_bytes." + key, expectedActorLayouts.GetValueOrDefault(key), actualActorLayouts.GetValueOrDefault(key));
foreach (string key in expectedActorDefinitions.Union(actualActorDefinitions))
    Check("actor_definition." + key, expectedActorDefinitions.Contains(key), actualActorDefinitions.Contains(key));
Check("actor_path_unresolved_source_keys", actorPaths["unresolved_source_record_keys"]!.GetValue<long>(),
    actorPaths["links"]!.AsArray().LongCount(e => e!["record"]!["source_key"] == null));
bool passed = mismatches == 0;
Console.WriteLine(JsonSerializer.Serialize(new {
    schema_version = 1, oracle = "Mutagen.Bethesda.Skyrim", version = "0.54.4",
    revision = "0188012c607ce8bb283d2704400d37737f089134", passed, distinct_records = distinctRecords,
    comparisons, mismatches, expected_edges = expectedEdges.Values.Sum(), independent_edges = actualEdges.Values.Sum(),
    expected_actor_edges = expectedActorEdges.Values.Sum(), independent_actor_edges = actualActorEdges.Values.Sum(),
    expected_actor_definitions = expectedActorDefinitions.Count, independent_actor_definitions = actualActorDefinitions.Count,
    actor_path_unresolved_source_keys = actorPaths["unresolved_source_record_keys"]!.GetValue<long>(),
    actor_graph_unresolved_links = actorPaths["unresolved_links"]!.AsArray().Count, sources, examples,
    scope = "Requested record identities/editor IDs/flags/VMAD presence, CELL flags, placed-reference containing cells, complete direct NAME/XESP/EFID edge multiset to missing-script owners, and complete reverse Skyrim SPLO/TPLT/LVLO/ACHR NAME actor-path edges with independently reconstructed exact field bytes. World ancestry, raw subrecord offsets, runtime winners, template inheritance, leveled selection and gameplay are not certified."
}, new JsonSerializerOptions { WriteIndented = true }));
return passed ? 0 : 2;
