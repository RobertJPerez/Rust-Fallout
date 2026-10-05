// Offline inspection harness. The upstream reader is never a runtime dependency.
using System.Buffers.Binary;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Mutagen.Bethesda;
using Mutagen.Bethesda.Fallout4;
using Mutagen.Bethesda.Plugins;
using Mutagen.Bethesda.Plugins.Binary.Parameters;
using Mutagen.Bethesda.Plugins.Binary.Streams;
using Mutagen.Bethesda.Plugins.Binary.Translations;
using Mutagen.Bethesda.Plugins.Masters;
using Mutagen.Bethesda.Plugins.Meta;

if (args.Length != 3) throw new ArgumentException("vmad-oracle <install-root> <scan-directory> <new-report.jsonl>");
using var output = new StreamWriter(new FileStream(args[2], FileMode.CreateNew), new UTF8Encoding(false));
Encoding.RegisterProvider(CodePagesEncodingProvider.Instance);
var metas = new Dictionary<string, ParsingMeta>();
int failures = 0;
foreach (var line in File.ReadLines(Path.Combine(args[1], "attachments.jsonl")))
{
    using var row = JsonDocument.Parse(line);
    var root = row.RootElement;
    var filename = root.GetProperty("file").GetString()!;
    if (Path.GetFileName(filename) != filename) throw new InvalidDataException("unsafe evidence filename");
    var data = File.ReadAllBytes(Path.Combine(args[1], filename));
    var hash = Convert.ToHexString(SHA256.HashData(data)).ToLowerInvariant();
    try
    {
        var origin = root.GetProperty("origin");
        var plugin = origin.GetProperty("plugin").GetString()!;
        if (!metas.TryGetValue(plugin, out var meta))
        {
            meta = ParsingMeta.Factory(new BinaryReadParameters { Parallel = false }, GameRelease.Fallout4,
                new ModPath(ModKey.FromFileName(plugin), Path.Combine(args[0], "Data", plugin)));
            metas.Add(plugin, meta);
        }
        meta.FormVersion = origin.GetProperty("record_version").GetUInt16();
        var rawIds = new RawIds(meta.MasterReferences is RawIds previous ? previous.Inner : meta.MasterReferences);
        meta.MasterReferences = rawIds;
        // Reconstruct only the subrecord envelope; all payload decoding is upstream.
        var framed = new byte[data.Length + 6];
        Encoding.ASCII.GetBytes("VMAD").CopyTo(framed, 0);
        BinaryPrimitives.WriteUInt16LittleEndian(framed.AsSpan(4), unchecked((ushort)data.Length));
        data.CopyTo(framed, 6);
        using var input = new MutagenMemoryReadStream(framed, meta);
        var frame = new MutagenFrame(input);
        var options = TypedParseParams.FromLengthOverride(data.Length);
        var core = VirtualMachineAdapter.CreateFromBinary(frame, options);
        bool suffix = input.Position < framed.Length;
        input.Position = 0;
        IAVirtualMachineAdapterGetter adapter = origin.GetProperty("record_kind").GetString() switch
        {
            "QUST" => QuestAdapter.CreateFromBinary(frame, options),
            "INFO" => DialogResponsesAdapter.CreateFromBinary(frame, options),
            "PACK" => PackageAdapter.CreateFromBinary(frame, options),
            "SCEN" => SceneAdapter.CreateFromBinary(frame, options),
            "PERK" => PerkAdapter.CreateFromBinary(frame, options),
            "TERM" => VirtualMachineAdapterIndexed.CreateFromBinary(frame, options),
            _ => VirtualMachineAdapter.CreateFromBinary(frame, options)
        };
        if (input.Position != framed.Length) throw new InvalidDataException($"unconsumed bytes at {input.Position}");
        var tokens = new Exporter(meta).Tokens(adapter, suffix);
        output.WriteLine(JsonSerializer.Serialize(new { file = filename, sha256 = hash, tokens, reference_normalizations = rawIds.Normalizations, error = (string?)null }));
    }
    catch (Exception e)
    {
        failures++;
        output.WriteLine(JsonSerializer.Serialize(new { file = filename, sha256 = hash, tokens = (object?)null, error = e.ToString() }));
    }
}
return failures == 0 ? 0 : 2;

// Preserve the raw uint supplied by the upstream decoder at its identity boundary.
// Its normal resolver can canonicalize out-of-range master slots to the current
// plugin. Structural comparison must neither normalize nor interpret those IDs.
sealed class RawIds(IReadOnlySeparatedMasterPackage inner) : IReadOnlySeparatedMasterPackage
{
    public IReadOnlySeparatedMasterPackage Inner => inner;
    public ModKey CurrentMod => inner.CurrentMod;
    public IReadOnlyMasterReferenceCollection Raw => inner.Raw;
    readonly Dictionary<uint, FormKey> keys = [];
    readonly Dictionary<FormKey, uint> ids = [];
    public Dictionary<uint, uint> Normalizations { get; } = [];
    public bool TryLookupModKey(ModKey key, bool reference, out MasterStyle style, out uint index)
        => inner.TryLookupModKey(key, reference, out style, out index);
    public FormKey GetFormKey(FormID id, bool reference)
    {
        var canonical = inner.GetFormID(inner.GetFormKey(id, reference)).Raw;
        if (canonical != id.Raw) Normalizations[id.Raw] = canonical;
        if (!keys.TryGetValue(id.Raw, out var key))
        {
            key = id.Raw == 0 ? FormKey.Null : new FormKey(ModKey.FromFileName("OracleRawIds.esp"), checked((uint)keys.Count + 1));
            keys.Add(id.Raw, key); ids.Add(key, id.Raw);
        }
        return key;
    }
    public FormID GetFormID(FormKey key) => new(ids[key]);
}

sealed class Exporter(ParsingMeta meta)
{
    readonly List<object?> t = [];
    static string Hex(string value) => Convert.ToHexString(Encoding.GetEncoding(1252).GetBytes(value)).ToLowerInvariant();
    void Obj(IScriptObjectPropertyGetter p) => t.AddRange([meta.MasterReferences.GetFormID(p.Object.FormKey).Raw, p.Alias, p.Unused]);
    void Properties(IEnumerable<IScriptPropertyGetter> input)
    {
        var props = input.ToList();
        t.Add(props.Count);
        foreach (var p in props)
        {
            byte type = p switch {
                IScriptObjectPropertyGetter => 1, IScriptStringPropertyGetter => 2,
                IScriptIntPropertyGetter => 3, IScriptFloatPropertyGetter => 4, IScriptBoolPropertyGetter => 5,
                IScriptStructPropertyGetter => 7, IScriptObjectListPropertyGetter => 11,
                IScriptStringListPropertyGetter => 12, IScriptIntListPropertyGetter => 13,
                IScriptFloatListPropertyGetter => 14, IScriptBoolListPropertyGetter => 15,
                IScriptStructListPropertyGetter => 17,
                IScriptVariablePropertyGetter or IScriptVariableListPropertyGetter => throw new NotSupportedException("variable property"),
                _ => 0 };
            t.AddRange([Hex(p.Name), (byte)p.Flags, type]);
            switch (p)
            {
                case IScriptObjectPropertyGetter v: Obj(v); break;
                case IScriptStringPropertyGetter v: t.Add(Hex(v.Data)); break;
                case IScriptIntPropertyGetter v: t.Add(v.Data); break;
                case IScriptFloatPropertyGetter v: t.Add(BitConverter.SingleToUInt32Bits(v.Data)); break;
                case IScriptBoolPropertyGetter v: t.Add(v.Data ? 1 : 0); break;
                case IScriptStructPropertyGetter v: Properties(v.Members.SelectMany(m => m.Properties)); break;
                case IScriptObjectListPropertyGetter v: t.Add(v.Objects.Count); foreach (var o in v.Objects) Obj(o); break;
                case IScriptStringListPropertyGetter v: t.Add(v.Data.Count); foreach (var s in v.Data) t.Add(Hex(s)); break;
                case IScriptIntListPropertyGetter v: t.Add(v.Data.Count); foreach (var n in v.Data) t.Add(n); break;
                case IScriptFloatListPropertyGetter v: t.Add(v.Data.Count); foreach (var n in v.Data) t.Add(BitConverter.SingleToUInt32Bits(n)); break;
                case IScriptBoolListPropertyGetter v: t.Add(v.Data.Count); foreach (var n in v.Data) t.Add(n ? 1 : 0); break;
                case IScriptStructListPropertyGetter v: t.Add(v.Structs.Count); foreach (var s in v.Structs) Properties(s.Members); break;
            }
        }
    }
    void Script(IScriptEntryGetter s)
    {
        t.AddRange([Hex(s.Name), s.Name.Length == 0 ? null : (object)(byte)s.Flags]);
        Properties(s.Properties);
    }
    void Scripts(IEnumerable<IScriptEntryGetter> s)
    {
        var list = s.ToList(); t.Add(list.Count); foreach (var item in list) Script(item);
    }
    void Event(uint index, IScriptFragmentGetter f) => t.AddRange([index, null, f.ExtraBindDataVersion, Hex(f.ScriptName), Hex(f.FragmentName)]);
    void Events(IScriptFragmentsGetter f)
    {
        var flags = (f.OnBegin != null ? 1 : 0) | (f.OnEnd != null ? 2 : 0);
        t.AddRange([f.ExtraBindDataVersion, flags]); Script(f.Script);
        t.Add((f.OnBegin != null ? 1 : 0) + (f.OnEnd != null ? 1 : 0));
        if (f.OnBegin is {} b) Event(0, b); if (f.OnEnd is {} e) Event(1, e);
    }
    static uint Packed(ushort lo, short hi) => lo | ((uint)(ushort)hi << 16);
    public List<object?> Tokens(IAVirtualMachineAdapterGetter a, bool suffix)
    {
        t.AddRange([a.Version, a.ObjectFormat]); Scripts(a.Scripts); t.Add(suffix);
        if (!suffix) return t;
        switch (a)
        {
            case IQuestAdapterGetter q:
                t.AddRange([q.ExtraBindDataVersion, null]); Script(q.Script); t.Add(q.Fragments.Count);
                foreach (var f in q.Fragments) t.AddRange([Packed(f.Stage, f.Unknown), unchecked((uint)f.StageIndex), unchecked((byte)f.Unknown2), Hex(f.ScriptName), Hex(f.FragmentName)]);
                t.Add(0); t.Add(q.Aliases.Count);
                foreach (var alias in q.Aliases) { Obj(alias.Property); t.AddRange([alias.Version, alias.ObjectFormat]); Scripts(alias.Scripts); }
                break;
            case IDialogResponsesAdapterGetter i:
                Events(i.ScriptFragments!); t.AddRange([0, 0]); break;
            case ISceneAdapterGetter s:
                var scene = s.ScriptFragments!; Events(scene); t.Add(scene.PhaseFragments.Count);
                foreach (var p in scene.PhaseFragments)
                    t.AddRange([(byte)p.Flags, (uint)p.Index | ((p.Unknown & 0x00FFFFFF) << 8), (byte)(p.Unknown >> 24), Hex(p.ScriptName), Hex(p.FragmentName)]);
                t.Add(0); break;
            case IPackageAdapterGetter p:
                var pack = p.ScriptFragments!;
                var flags = (pack.OnBegin != null ? 1 : 0) | (pack.OnEnd != null ? 2 : 0) | (pack.OnChange != null ? 4 : 0);
                t.AddRange([pack.ExtraBindDataVersion, flags]); Script(pack.Script);
                t.Add((pack.OnBegin != null ? 1 : 0) + (pack.OnEnd != null ? 1 : 0) + (pack.OnChange != null ? 1 : 0));
                if (pack.OnBegin is {} b) Event(0, b); if (pack.OnEnd is {} e) Event(1, e); if (pack.OnChange is {} c) Event(2, c);
                t.AddRange([0, 0]); break;
            case IPerkAdapterGetter p:
                var perk = p.ScriptFragments!; t.AddRange([perk.ExtraBindDataVersion, null]); Script(perk.Script); t.Add(perk.Fragments.Count);
                foreach (var f in perk.Fragments) t.AddRange([Packed(f.Index, f.Unknown), null, unchecked((byte)f.Unknown2), Hex(f.ScriptName), Hex(f.FragmentName)]);
                t.AddRange([0, 0]); break;
            case IVirtualMachineAdapterIndexedGetter i:
                var indexed = i.ScriptFragments!; t.AddRange([indexed.ExtraBindDataVersion, null]); Script(indexed.Script); t.Add(indexed.Fragments.Count);
                foreach (var f in indexed.Fragments) t.AddRange([Packed(f.FragmentIndex, f.Unknown), null, unchecked((byte)f.Unknown2), Hex(f.ScriptName), Hex(f.FragmentName)]);
                t.AddRange([0, 0]); break;
            default: throw new InvalidDataException("unexpected fragment section");
        }
        return t;
    }
}
