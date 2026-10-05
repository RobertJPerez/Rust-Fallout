// Separate GPL-3.0-only validation executable. Never linked into the Rust runtime.
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Mutagen.Bethesda.Plugins;
using Mutagen.Bethesda.Plugins.Records;
using Mutagen.Bethesda.Skyrim;

if (args.Length != 2) throw new ArgumentException("usage: vmad-oracle PLUGIN RUST_BINDINGS_JSONL");
Encoding.RegisterProvider(CodePagesEncodingProvider.Instance);
var textEncoding = Encoding.GetEncoding(1252, EncoderFallback.ExceptionFallback, DecoderFallback.ExceptionFallback);
var evidence = new Dictionary<uint, JsonObject>();
JsonObject? source = null;
JsonObject? complete = null;
foreach (var line in File.ReadLines(args[1]))
{
    var row = JsonNode.Parse(line)!.AsObject();
    if (complete != null) throw new InvalidDataException("row after completeness trailer");
    switch (row["type"]!.GetValue<string>())
    {
        case "source" when source == null && evidence.Count == 0: source = row; break;
        case "binding" when source != null: evidence.Add(row["form_id"]!.GetValue<uint>(), row); break;
        case "complete" when source != null: complete = row; break;
        default: throw new InvalidDataException("invalid evidence row order or type");
    }
}
if (source == null || complete == null || source["schema_version"]!.GetValue<int>() != 1)
    throw new InvalidDataException("missing/unsupported source header or completeness trailer");
if (complete["summary"]!["bindings"]!.GetValue<int>() != evidence.Count || complete["summary"]!["failures"]!.GetValue<int>() != 0)
    throw new InvalidDataException("incomplete/failed Rust export");
using var input = new FileStream(args[0], FileMode.Open, FileAccess.Read, FileShare.Read);
var sha256 = Convert.ToHexStringLower(SHA256.HashData(input));
if (source["sha256"]!.GetValue<string>() != sha256 || source["bytes"]!.GetValue<long>() != input.Length ||
    !string.Equals(source["file"]!.GetValue<string>(), Path.GetFileName(args[0]), StringComparison.OrdinalIgnoreCase))
    throw new InvalidDataException("evidence does not describe this source file");
input.Position = 0;
var modKey = ModKey.FromFileName(Path.GetFileName(args[0]));
using var mod = SkyrimMod.CreateFromBinaryOverlay(input, SkyrimRelease.SkyrimSE, modKey);
var masters = mod.ModHeader.MasterReferences.Select(m => m.Master).Append(modKey).ToArray();
long comparisons = 0, mismatches = 0, matchedBindings = 0, actualBindings = 0, decodedTails = 0;
var examples = new List<object>();
var seen = new HashSet<uint>();
var kinds = new SortedDictionary<string, int>();

void Mismatch(string path, object? rust, object? independent)
{
    mismatches++;
    if (examples.Count < 64) examples.Add(new { path, rust, independent });
}
void Compare(string path, JsonNode? rust, JsonNode? independent)
{
    comparisons++;
    if (rust is JsonObject a && independent is JsonObject b)
    {
        foreach (var key in a.Select(p => p.Key).Union(b.Select(p => p.Key)))
        {
            if (!a.ContainsKey(key) || !b.ContainsKey(key)) Mismatch(path + "." + key, a.ContainsKey(key), b.ContainsKey(key));
            else Compare(path + "." + key, a[key], b[key]);
        }
    }
    else if (rust is JsonArray x && independent is JsonArray y)
    {
        if (x.Count != y.Count) Mismatch(path + ".count", x.Count, y.Count);
        for (int i = 0; i < Math.Min(x.Count, y.Count); i++) Compare($"{path}[{i}]", x[i], y[i]);
    }
    else if (!JsonNode.DeepEquals(rust, independent)) Mismatch(path, rust?.DeepClone(), independent?.DeepClone());
}
JsonNode? Node(object? value) => JsonSerializer.SerializeToNode(value);
string Text(JsonNode? bytes) => textEncoding.GetString(bytes!.AsArray().Select(n => n!.GetValue<byte>()).ToArray());
uint Raw(FormKey key)
{
    if (key.IsNull) return 0;
    int index = Array.IndexOf(masters, key.ModKey);
    if (index < 0 || index > 255 || key.ID > 0xFFFFFF) throw new InvalidDataException("unrepresentable source FormKey");
    return (uint)index << 24 | key.ID;
}
JsonNode ObjectValue(IScriptObjectPropertyGetter p) => Node(new { form_id = Raw(p.Object.FormKey), alias = p.Alias, unused = new int[] {p.Unused & 255, p.Unused >> 8} })!;
JsonNode Tag(string name, object? value) => new JsonObject { [name] = Node(value) };
JsonNode ArrayValue(int element, IEnumerable<JsonNode> values) => new JsonObject {
    ["Array"] = new JsonObject { ["element_type"] = element, ["values"] = new JsonArray(values.ToArray()) }
};
JsonNode Value(IScriptPropertyGetter p) => p switch
{
    IScriptObjectPropertyGetter v => new JsonObject { ["Object"] = ObjectValue(v) },
    IScriptStringPropertyGetter v => Tag("String", v.Data),
    IScriptIntPropertyGetter v => Tag("Int", v.Data),
    IScriptFloatPropertyGetter v => Tag("FloatBits", BitConverter.SingleToUInt32Bits(v.Data)),
    IScriptBoolPropertyGetter v => Tag("BoolByte", v.Data ? 1 : 0),
    IScriptObjectListPropertyGetter v => ArrayValue(1, v.Objects.Select(o => new JsonObject { ["Object"] = ObjectValue(o) })),
    IScriptStringListPropertyGetter v => ArrayValue(2, v.Data.Select(s => Tag("String", s))),
    IScriptIntListPropertyGetter v => ArrayValue(3, v.Data.Select(s => Tag("Int", s))),
    IScriptFloatListPropertyGetter v => ArrayValue(4, v.Data.Select(s => Tag("FloatBits", BitConverter.SingleToUInt32Bits(s)))),
    IScriptBoolListPropertyGetter v => ArrayValue(5, v.Data.Select(s => Tag("BoolByte", s ? 1 : 0))),
    _ when p.GetType() == typeof(ScriptProperty) => JsonValue.Create("None"),
    _ => throw new InvalidDataException("unsupported independent property type " + p.GetType())
};
JsonArray Scripts(IReadOnlyList<IScriptEntryGetter> scripts) => new(scripts.Select(s => (JsonNode)new JsonObject {
    ["name"] = s.Name, ["status"] = (byte)s.Flags,
    ["properties"] = new JsonArray(s.Properties.Select(p => (JsonNode)new JsonObject {
        ["name"] = p.Name, ["status"] = (byte)p.Flags, ["value"] = Value(p)
    }).ToArray())
}).ToArray());
JsonNode Fragment(JsonNode selector, byte unknown, string script, string function) => new JsonObject {
    ["selector"] = selector, ["unknown"] = unknown, ["script_name"] = script, ["function_name"] = function
};
JsonObject Extension(int version, int? flags, string file, JsonArray fragments, JsonArray? aliases = null) => new() {
    ["extra_bind_version"] = version, ["flags"] = Node(flags), ["file_name"] = file,
    ["fragments"] = fragments, ["aliases"] = aliases ?? new JsonArray()
};
JsonObject Events(int version, string file, params IScriptFragmentGetter?[] events)
{
    int flags = 0;
    var fragments = new JsonArray();
    for (int i = 0; i < events.Length; i++) if (events[i] is {} f)
    {
        flags |= 1 << i;
        fragments.Add(Fragment(Tag("FlagBit", i), unchecked((byte)f.ExtraBindDataVersion), f.ScriptName, f.FragmentName));
    }
    return Extension(version, flags, file, fragments);
}
JsonObject? Tail(IAVirtualMachineAdapterGetter a)
{
    switch (a)
    {
        case IQuestAdapterGetter q when !q.Versioning.HasFlag(QuestAdapter.VersioningBreaks.Break0):
            return Extension(q.ExtraBindDataVersion, null, q.FileName,
                new JsonArray(q.Fragments.Select(f => Fragment(Tag("Quest", new {
                    stage = (uint)f.Stage | (uint)unchecked((ushort)f.Unknown) << 16, log_entry = unchecked((uint)f.StageIndex)
                }), unchecked((byte)f.Unknown2), f.ScriptName, f.FragmentName)).ToArray()),
                new JsonArray(q.Aliases.Select(alias => (JsonNode)new JsonObject {
                    ["object"] = ObjectValue(alias.Property), ["version"] = alias.Version,
                    ["object_format"] = alias.ObjectFormat, ["scripts"] = Scripts(alias.Scripts)
                }).ToArray()));
        case IDialogResponsesAdapterGetter { ScriptFragments: {} f }:
            return Events(unchecked((byte)f.ExtraBindDataVersion), f.FileName, f.OnBegin, f.OnEnd);
        case IPackageAdapterGetter { ScriptFragments: {} f }:
            return Events(f.ExtraBindDataVersion, f.FileName, f.OnBegin, f.OnEnd, f.OnChange);
        case IPerkAdapterGetter { ScriptFragments: {} f }:
            return Extension(f.ExtraBindDataVersion, null, f.FileName,
                new JsonArray(f.Fragments.Select(p => Fragment(Tag("PerkIndex", (uint)p.FragmentIndex | (uint)unchecked((ushort)p.Unknown) << 16),
                    unchecked((byte)p.Unknown2), p.ScriptName, p.FragmentName)).ToArray()));
        case ISceneAdapterGetter { ScriptFragments: {} f }:
            var result = Events(unchecked((byte)f.ExtraBindDataVersion), f.FileName, f.OnBegin, f.OnEnd);
            foreach (var phase in f.PhaseFragments)
                result["fragments"]!.AsArray().Add(Fragment(Tag("ScenePhase", new {
                    flags = (byte)phase.Flags, index = (uint)phase.Index | (phase.Unknown & 0xFFFFFF) << 8
                }), (byte)(phase.Unknown >> 24), phase.ScriptName, phase.FragmentName));
            return result;
        default: return null;
    }
}
// Normalize representation only: offsets are Rust evidence, and strings use the
// independent library's nonlocalized Windows-1252 encoding. Preserve all order.
JsonNode RustValue(JsonNode value)
{
    var result = value.DeepClone();
    if (result is JsonObject obj)
    {
        if (obj.ContainsKey("String")) obj["String"] = Text(obj["String"]);
        if (obj.ContainsKey("Array")) obj["Array"]!["values"] = new JsonArray(obj["Array"]!["values"]!.AsArray().Select(v => RustValue(v!)).ToArray());
    }
    return result;
}
JsonArray RustScripts(JsonNode scripts) => new(scripts.AsArray().Select(s => (JsonNode)new JsonObject {
    ["name"] = Text(s!["name"]), ["status"] = s["status"]!.DeepClone(),
    ["properties"] = new JsonArray(s["properties"]!.AsArray().Select(p => (JsonNode)new JsonObject {
        ["name"] = Text(p!["name"]), ["status"] = p["status"]!.DeepClone(), ["value"] = RustValue(p["value"]!)
    }).ToArray())
}).ToArray());
JsonObject RustAttachment(JsonNode a)
{
    JsonObject? tail = null;
    if (a["tail"] is JsonObject tails)
    {
        var t = tails["Decoded"] ?? throw new InvalidDataException("Rust tail is unsupported");
        tail = Extension(t["extra_bind_version"]!.GetValue<int>(), t["flags"]?.GetValue<int>(), Text(t["file_name"]),
            new JsonArray(t["fragments"]!.AsArray().Select(f => Fragment(f!["selector"]!.DeepClone(), f["unknown"]!.GetValue<byte>(), Text(f["script_name"]), Text(f["function_name"]))).ToArray()),
            new JsonArray(t["aliases"]!.AsArray().Select(alias => (JsonNode)new JsonObject {
                ["object"] = alias!["object"]!.DeepClone(), ["version"] = alias["version"]!.DeepClone(),
                ["object_format"] = alias["object_format"]!.DeepClone(), ["scripts"] = RustScripts(alias["scripts"]!)
            }).ToArray()));
    }
    else if (a["tail"]!.GetValue<string>() != "Absent") throw new InvalidDataException("unknown Rust tail");
    return new JsonObject { ["version"] = a["version"]!.DeepClone(), ["object_format"] = a["object_format"]!.DeepClone(), ["scripts"] = RustScripts(a["scripts"]!), ["tail"] = tail };
}

foreach (var record in mod.EnumerateMajorRecords().OfType<IHaveVirtualMachineAdapterGetter>())
{
    if (record.VirtualMachineAdapter is not {} adapter) continue;
    actualBindings++;
    uint id = Raw(record.FormKey);
    if (!seen.Add(id)) throw new InvalidDataException("duplicate independent physical FormID");
    string context = $"{modKey}:{id:X8}";
    if (!evidence.TryGetValue(id, out var row)) { Mismatch(context, "missing Rust binding", "VMAD present"); continue; }
    var before = mismatches;
    try
    {
        var tail = Tail(adapter);
        if (tail != null) decodedTails++;
        string kind = row["kind"]!.GetValue<string>();
        kinds[kind] = kinds.GetValueOrDefault(kind) + 1;
        Compare(context, RustAttachment(row["attachment"]!), new JsonObject {
            ["version"] = adapter.Version, ["object_format"] = adapter.ObjectFormat,
            ["scripts"] = Scripts(adapter.Scripts), ["tail"] = tail
        });
    }
    catch (Exception e) { Mismatch(context, "comparison failed", e.Message); }
    if (mismatches == before) matchedBindings++;
}
foreach (var id in evidence.Keys.Except(seen)) Mismatch($"{modKey}:{id:X8}", "VMAD present", "missing independent binding");
Compare("complete.decoded_tails", complete["summary"]!["decoded_tails"], Node(decodedTails));
bool passed = mismatches == 0 && matchedBindings == evidence.Count && actualBindings == evidence.Count;
Console.WriteLine(JsonSerializer.Serialize(new {
    schema_version = 1, oracle = "Mutagen.Bethesda.Skyrim", version = "0.54.4",
    revision = "0188012c607ce8bb283d2704400d37737f089134", file = Path.GetFileName(args[0]), sha256,
    passed, rust_bindings = evidence.Count, independent_bindings = actualBindings, matched_bindings = matchedBindings,
    decoded_tails = decodedTails, comparisons, mismatches, kinds, examples,
    scope = "Ordered VMAD primary/alias scripts, properties, object IDs/padding, numeric bits, fragment selectors/names, extra-bind versions and flags; Windows-1252 text. Source offsets/raw VMAD hashes and gameplay are not independently certified."
}, new JsonSerializerOptions { WriteIndented = true }));
return passed ? 0 : 2;
