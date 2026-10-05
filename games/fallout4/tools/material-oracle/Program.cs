using System.Security.Cryptography;
using System.Runtime.Serialization.Json;
using System.Text.Json;
using System.Text.Json.Serialization;
using MaterialLib;

const string ReferenceRevision = "21411873f17454a2442d6785d533499e14c63adb";
const long MaxManifestBytes = 64 * 1024 * 1024;
const long MaxMaterialBytes = 1024 * 1024;
const int MaxMaterials = 100_000;

if (args.Length != 3)
{
    Console.Error.WriteLine("usage: material-oracle <extraction-directory> <jsonl-output> <summary-output>");
    return 1;
}

var root = Path.GetFullPath(args[0]);
var manifestPath = Path.Combine(root, "manifest.json");
var completionPath = Path.Combine(root, "complete.json");
var manifestInfo = new FileInfo(manifestPath);
if (!manifestInfo.Exists || manifestInfo.Length > MaxManifestBytes)
    throw new InvalidDataException("missing or oversized material manifest");

var manifestBytes = File.ReadAllBytes(manifestPath);
var manifestHash = Hex(SHA256.HashData(manifestBytes));
var manifest = JsonSerializer.Deserialize<Manifest>(manifestBytes)
    ?? throw new InvalidDataException("empty material manifest");
var completion = JsonSerializer.Deserialize<Completion>(File.ReadAllBytes(completionPath))
    ?? throw new InvalidDataException("empty material completion marker");
if (completion.ManifestSha256 != manifestHash || completion.Bgsm != manifest.Bgsm ||
    completion.Bgem != manifest.Bgem || completion.ArchivesScanned != manifest.ArchivesScanned ||
    completion.ArchivesWithMaterials != manifest.ArchivesWithMaterials ||
    manifest.Materials.Count != manifest.Bgsm + manifest.Bgem || manifest.Materials.Count > MaxMaterials)
    throw new InvalidDataException("material manifest/completion mismatch");

var outputPath = Path.GetFullPath(args[1]);
var summaryPath = Path.GetFullPath(args[2]);
Directory.CreateDirectory(Path.GetDirectoryName(outputPath)!);
Directory.CreateDirectory(Path.GetDirectoryName(summaryPath)!);
var tempOutput = outputPath + ".partial";
if (File.Exists(outputPath) || File.Exists(summaryPath) || File.Exists(tempOutput))
    throw new IOException("oracle output path already exists; choose a new evidence directory");

int parsed = 0, jsonParsed = 0, trailing = 0, errors = 0;
var versions = new SortedDictionary<string, int>(StringComparer.Ordinal);
var temporaryOutputCreated = false;
try
{
    using (var output = new StreamWriter(new FileStream(tempOutput, FileMode.CreateNew, FileAccess.Write, FileShare.None)))
    {
        temporaryOutputCreated = true;
        foreach (var row in manifest.Materials)
        {
            var sourcePath = Path.Combine(root, row.ExtractedFile);
            var info = new FileInfo(sourcePath);
            string state;
            string? error = null;
            uint? version = null;
            long consumed = 0;
            List<TextureSlot> textures = [];
            List<OracleField> fields = [];
            var materialSha = "";
            try
            {
                if (!info.Exists || info.Length > MaxMaterialBytes || info.Length != row.Bytes)
                    throw new InvalidDataException("missing, oversized, or length-mismatched extracted material");
                if (!StringComparer.Ordinal.Equals(Path.GetFileName(row.ExtractedFile), row.ExtractedFile))
                    throw new InvalidDataException("extracted material name escapes the evidence directory");
                var bytes = File.ReadAllBytes(sourcePath);
                materialSha = Hex(SHA256.HashData(bytes));
                if (!StringComparer.Ordinal.Equals(materialSha, row.Sha256))
                    throw new InvalidDataException("extracted payload hash differs from manifest");

                using var stream = new MemoryStream(bytes, writable: false);
                var jsonFormat = bytes.Length > 0 && (bytes[0] == (byte)'{' || bytes[0] == (byte)'[');
                if (row.Kind.Equals("bgsm", StringComparison.OrdinalIgnoreCase))
                {
                    BGSM material;
                    if (jsonFormat)
                    {
                        material = (BGSM)(new DataContractJsonSerializer(typeof(BGSM)).ReadObject(stream)
                            ?? throw new InvalidDataException("empty BGSM JSON"));
                        consumed = stream.Position;
                    }
                    else
                    {
                        material = new BGSM();
                        using var reader = new BinaryReader(stream);
                        try { material.Deserialize(reader); }
                        finally
                        {
                            consumed = stream.Position;
                            if (consumed >= 8) version = material.Version;
                        }
                    }
                    textures =
                    [
                        Slot(nameof(material.DiffuseTexture), material.DiffuseTexture),
                        Slot(nameof(material.NormalTexture), material.NormalTexture),
                        Slot(nameof(material.SmoothSpecTexture), material.SmoothSpecTexture),
                        Slot(nameof(material.GreyscaleTexture), material.GreyscaleTexture),
                        Slot(nameof(material.EnvmapTexture), material.EnvmapTexture),
                        Slot(nameof(material.GlowTexture), material.GlowTexture),
                        Slot(nameof(material.InnerLayerTexture), material.InnerLayerTexture),
                        Slot(nameof(material.WrinklesTexture), material.WrinklesTexture),
                        Slot(nameof(material.DisplacementTexture), material.DisplacementTexture),
                        Slot(nameof(material.SpecularTexture), material.SpecularTexture),
                        Slot(nameof(material.LightingTexture), material.LightingTexture),
                        Slot(nameof(material.FlowTexture), material.FlowTexture),
                        Slot(nameof(material.DistanceFieldAlphaTexture), material.DistanceFieldAlphaTexture),
                    ];
                    if (!jsonFormat) fields = BgsmFields(material);
                }
                else if (row.Kind.Equals("bgem", StringComparison.OrdinalIgnoreCase))
                {
                    BGEM material;
                    if (jsonFormat)
                    {
                        material = (BGEM)(new DataContractJsonSerializer(typeof(BGEM)).ReadObject(stream)
                            ?? throw new InvalidDataException("empty BGEM JSON"));
                        consumed = stream.Position;
                    }
                    else
                    {
                        material = new BGEM();
                        using var reader = new BinaryReader(stream);
                        try { material.Deserialize(reader); }
                        finally
                        {
                            consumed = stream.Position;
                            if (consumed >= 8) version = material.Version;
                        }
                    }
                    textures =
                    [
                        Slot(nameof(material.BaseTexture), material.BaseTexture),
                        Slot(nameof(material.GrayscaleTexture), material.GrayscaleTexture),
                        Slot(nameof(material.EnvmapTexture), material.EnvmapTexture),
                        Slot(nameof(material.NormalTexture), material.NormalTexture),
                        Slot(nameof(material.EnvmapMaskTexture), material.EnvmapMaskTexture),
                        Slot(nameof(material.SpecularTexture), material.SpecularTexture),
                        Slot(nameof(material.LightingTexture), material.LightingTexture),
                        Slot(nameof(material.GlowTexture), material.GlowTexture),
                    ];
                    if (!jsonFormat) fields = BgemFields(material);
                }
                else
                {
                    throw new InvalidDataException("unsupported manifest material kind");
                }

                state = consumed == bytes.Length ? "parsed" : "trailing_bytes";
                if (state == "parsed")
                {
                    if (jsonFormat) jsonParsed++; else parsed++;
                }
                else trailing++;
            }
            catch (Exception ex)
            {
                state = "error";
                error = ex.GetType().Name + ": " + ex.Message;
                errors++;
            }

            if (version is uint v)
                versions[v.ToString(System.Globalization.CultureInfo.InvariantCulture)] =
                    versions.GetValueOrDefault(v.ToString(System.Globalization.CultureInfo.InvariantCulture)) + 1;
            var result = new MaterialResult(
                row.Archive, row.ArchiveSha256, row.EntryIndex, row.MemberNameBytesHex, row.Kind,
                row.Bytes, row.Sha256, row.ExtractedFile, state, error, version, consumed,
                materialSha, textures.Where(t => t.Value.Length != 0).ToArray(), fields.ToArray());
            output.WriteLine(JsonSerializer.Serialize(result));
        }
    }
    File.Move(tempOutput, outputPath);
    var jsonlHash = HashFile(outputPath);
    var summary = new Summary(1, ReferenceRevision, manifestHash, jsonlHash,
        manifest.ArchivesScanned, manifest.ArchivesWithMaterials, manifest.Bgsm, manifest.Bgem,
        parsed, jsonParsed, trailing, errors, versions);
    using var summaryFile = new FileStream(summaryPath, FileMode.CreateNew, FileAccess.Write, FileShare.None);
    JsonSerializer.Serialize(summaryFile, summary, new JsonSerializerOptions { WriteIndented = true });
    summaryFile.WriteByte((byte)'\n');
    return errors == 0 && trailing == 0 ? 0 : 2;
}
catch
{
    if (temporaryOutputCreated)
        Console.Error.WriteLine($"incomplete output retained: {tempOutput}");
    throw;
}

static TextureSlot Slot(string name, string? value) => new(name, value ?? "");
static OracleField BoolField(string name, bool value) => new(name, "bool", JsonSerializer.SerializeToElement(value));
static OracleField ByteField(string name, byte value) => new(name, "u8", JsonSerializer.SerializeToElement(value));
static OracleField FloatBitsField(string name, float value) => new(name, "f32_bits", JsonSerializer.SerializeToElement(BitConverter.SingleToUInt32Bits(value)));
static OracleField StringField(string name, string? value) => new(name, "string", JsonSerializer.SerializeToElement(value ?? ""));
static OracleField EnumField(string name, string value) => new(name, "enum", JsonSerializer.SerializeToElement(value));
static OracleField ColorField(string name, uint value) => new(name, "color_rgb", JsonSerializer.SerializeToElement(value));
static List<OracleField> BaseFields(BaseMaterialFile material) =>
[
    BoolField("TileU", material.TileU),
    BoolField("TileV", material.TileV),
    FloatBitsField("UOffset", material.UOffset),
    FloatBitsField("VOffset", material.VOffset),
    FloatBitsField("UScale", material.UScale),
    FloatBitsField("VScale", material.VScale),
    FloatBitsField("Alpha", material.Alpha),
    EnumField("AlphaBlendMode", material.AlphaBlendMode.ToString()),
    ByteField("AlphaTestRef", material.AlphaTestRef),
    BoolField("AlphaTest", material.AlphaTest),
    BoolField("ZBufferWrite", material.ZBufferWrite),
    BoolField("ZBufferTest", material.ZBufferTest),
    BoolField("ScreenSpaceReflections", material.ScreenSpaceReflections),
    BoolField("WetnessControlScreenSpaceReflections", material.WetnessControlScreenSpaceReflections),
    BoolField("Decal", material.Decal),
    BoolField("TwoSided", material.TwoSided),
    BoolField("DecalNoFade", material.DecalNoFade),
    BoolField("NonOccluder", material.NonOccluder),
    BoolField("Refraction", material.Refraction),
    BoolField("RefractionFalloff", material.RefractionFalloff),
    FloatBitsField("RefractionPower", material.RefractionPower),
    BoolField("EnvironmentMapping", material.EnvironmentMapping),
    FloatBitsField("EnvironmentMappingMaskScale", material.EnvironmentMappingMaskScale),
    BoolField("GrayscaleToPaletteColor", material.GrayscaleToPaletteColor),
];
static List<OracleField> BgsmFields(BGSM material)
{
    var fields = BaseFields(material);
    fields.AddRange([
        BoolField("EnableEditorAlphaRef", material.EnableEditorAlphaRef),
        BoolField("RimLighting", material.RimLighting),
        FloatBitsField("RimPower", material.RimPower),
        FloatBitsField("BackLightPower", material.BackLightPower),
        BoolField("SubsurfaceLighting", material.SubsurfaceLighting),
        FloatBitsField("SubsurfaceLightingRolloff", material.SubsurfaceLightingRolloff),
        BoolField("SpecularEnabled", material.SpecularEnabled),
        ColorField("SpecularColor", material.SpecularColor),
        FloatBitsField("SpecularMult", material.SpecularMult),
        FloatBitsField("Smoothness", material.Smoothness),
        FloatBitsField("FresnelPower", material.FresnelPower),
        FloatBitsField("WetnessControlSpecScale", material.WetnessControlSpecScale),
        FloatBitsField("WetnessControlSpecPowerScale", material.WetnessControlSpecPowerScale),
        FloatBitsField("WetnessControlSpecMinvar", material.WetnessControlSpecMinvar),
        FloatBitsField("WetnessControlEnvMapScale", material.WetnessControlEnvMapScale),
        FloatBitsField("WetnessControlFresnelPower", material.WetnessControlFresnelPower),
        FloatBitsField("WetnessControlMetalness", material.WetnessControlMetalness),
        StringField("RootMaterialPath", material.RootMaterialPath),
        BoolField("AnisoLighting", material.AnisoLighting),
        BoolField("EmitEnabled", material.EmitEnabled),
    ]);
    if (material.EmitEnabled) fields.Add(ColorField("EmittanceColor", material.EmittanceColor));
    fields.AddRange([
        FloatBitsField("EmittanceMult", material.EmittanceMult),
        BoolField("ModelSpaceNormals", material.ModelSpaceNormals),
        BoolField("ExternalEmittance", material.ExternalEmittance),
        BoolField("BackLighting", material.BackLighting),
        BoolField("ReceiveShadows", material.ReceiveShadows),
        BoolField("HideSecret", material.HideSecret),
        BoolField("CastShadows", material.CastShadows),
        BoolField("DissolveFade", material.DissolveFade),
        BoolField("AssumeShadowmask", material.AssumeShadowmask),
        BoolField("Glowmap", material.Glowmap),
        BoolField("EnvironmentMappingWindow", material.EnvironmentMappingWindow),
        BoolField("EnvironmentMappingEye", material.EnvironmentMappingEye),
        BoolField("Hair", material.Hair),
        ColorField("HairTintColor", material.HairTintColor),
        BoolField("Tree", material.Tree),
        BoolField("Facegen", material.Facegen),
        BoolField("SkinTint", material.SkinTint),
        BoolField("Tessellate", material.Tessellate),
        FloatBitsField("DisplacementTextureBias", material.DisplacementTextureBias),
        FloatBitsField("DisplacementTextureScale", material.DisplacementTextureScale),
        FloatBitsField("TessellationPnScale", material.TessellationPnScale),
        FloatBitsField("TessellationBaseFactor", material.TessellationBaseFactor),
        FloatBitsField("TessellationFadeDistance", material.TessellationFadeDistance),
        FloatBitsField("GrayscaleToPaletteScale", material.GrayscaleToPaletteScale),
        BoolField("SkewSpecularAlpha", material.SkewSpecularAlpha),
    ]);
    return fields;
}
static List<OracleField> BgemFields(BGEM material)
{
    var fields = BaseFields(material);
    fields.AddRange([
        BoolField("BloodEnabled", material.BloodEnabled),
        BoolField("EffectLightingEnabled", material.EffectLightingEnabled),
        BoolField("FalloffEnabled", material.FalloffEnabled),
        BoolField("FalloffColorEnabled", material.FalloffColorEnabled),
        BoolField("GrayscaleToPaletteAlpha", material.GrayscaleToPaletteAlpha),
        BoolField("SoftEnabled", material.SoftEnabled),
        ColorField("BaseColor", material.BaseColor),
        FloatBitsField("BaseColorScale", material.BaseColorScale),
        FloatBitsField("FalloffStartAngle", material.FalloffStartAngle),
        FloatBitsField("FalloffStopAngle", material.FalloffStopAngle),
        FloatBitsField("FalloffStartOpacity", material.FalloffStartOpacity),
        FloatBitsField("FalloffStopOpacity", material.FalloffStopOpacity),
        FloatBitsField("LightingInfluence", material.LightingInfluence),
        ByteField("EnvmapMinLOD", material.EnvmapMinLOD),
        FloatBitsField("SoftDepth", material.SoftDepth),
    ]);
    return fields;
}
static string Hex(byte[] bytes) => Convert.ToHexString(bytes).ToLowerInvariant();
static string HashFile(string path)
{
    using var stream = File.OpenRead(path);
    return Hex(SHA256.HashData(stream));
}

record TextureSlot(string Field, string Value);
record OracleField(string Name, string Kind, JsonElement Value);
record MaterialResult(
    string Archive, string ArchiveSha256, int EntryIndex, string MemberNameBytesHex,
    string Kind, int Bytes, string Sha256, string ExtractedFile, string State,
    string? Error, uint? Version, long Consumed, string ExtractedSha256,
    TextureSlot[] Textures, OracleField[] Fields);
record Summary(
    int SchemaVersion, string ReferenceRevision, string ManifestSha256, string ResultsSha256,
    int ArchivesScanned, int ArchivesWithMaterials, int Bgsm, int Bgem,
    int Parsed, int JsonParsed, int TrailingBytes, int Errors, SortedDictionary<string, int> Versions);
record Manifest(
    [property: JsonPropertyName("archives_scanned")] int ArchivesScanned,
    [property: JsonPropertyName("archives_with_materials")] int ArchivesWithMaterials,
    [property: JsonPropertyName("bgsm")] int Bgsm,
    [property: JsonPropertyName("bgem")] int Bgem,
    [property: JsonPropertyName("materials")] List<MaterialRow> Materials);
record MaterialRow(
    [property: JsonPropertyName("archive")] string Archive,
    [property: JsonPropertyName("archive_sha256")] string ArchiveSha256,
    [property: JsonPropertyName("entry_index")] int EntryIndex,
    [property: JsonPropertyName("member_name_bytes_hex")] string MemberNameBytesHex,
    [property: JsonPropertyName("kind")] string Kind,
    [property: JsonPropertyName("bytes")] int Bytes,
    [property: JsonPropertyName("sha256")] string Sha256,
    [property: JsonPropertyName("extracted_file")] string ExtractedFile);
record Completion(
    [property: JsonPropertyName("manifest_sha256")] string ManifestSha256,
    [property: JsonPropertyName("archives_scanned")] int ArchivesScanned,
    [property: JsonPropertyName("archives_with_materials")] int ArchivesWithMaterials,
    [property: JsonPropertyName("bgsm")] int Bgsm,
    [property: JsonPropertyName("bgem")] int Bgem);
