// Original offline reader for our native container, not Bethesda .fos files.
// It validates extents and checksums; canonical JSON semantics belong to the
// Rust restore tests. No game code is loaded or executed.
#include "../oracle-common/record_source.hpp"
using namespace fallout_records;

static std::string hex(const Bytes& bytes) {
    std::ostringstream out; out<<std::hex<<std::setfill('0');
    for (const auto byte:bytes) out<<std::setw(2)<<unsigned(byte); return out.str();
}
struct NativeFile {
    HANDLE file=INVALID_HANDLE_VALUE;
    Bytes bytes;
    explicit NativeFile(const std::filesystem::path& path) {
        file=CreateFileW(path.c_str(),GENERIC_READ,FILE_SHARE_READ,nullptr,OPEN_EXISTING,FILE_ATTRIBUTE_NORMAL,nullptr);
        if (file==INVALID_HANDLE_VALUE) throw std::runtime_error("native file read lock");
        try {
            LARGE_INTEGER size{};
            if (!GetFileSizeEx(file,&size) || size.QuadPart<232 || size.QuadPart>64LL*1024*1024+232) throw std::runtime_error("native file budget");
            bytes.resize(static_cast<size_t>(size.QuadPart));
            for (size_t cursor=0;cursor<bytes.size();) {
                const auto count=static_cast<DWORD>(std::min<size_t>(65536,bytes.size()-cursor)); DWORD read=0;
                if (!ReadFile(file,bytes.data()+cursor,count,&read,nullptr) || read!=count) throw std::runtime_error("native exact file read");
                cursor+=read;
            }
        } catch (...) { CloseHandle(file); file=INVALID_HANDLE_VALUE; throw; }
    }
    ~NativeFile() { if (file!=INVALID_HANDLE_VALUE) CloseHandle(file); }
};
struct Reader {
    const Bytes& bytes; size_t cursor=0;
    Bytes take(size_t count) {
        if (cursor>bytes.size() || count>bytes.size()-cursor) throw std::runtime_error("native container extent");
        Bytes out(bytes.begin()+cursor,bytes.begin()+cursor+count); cursor+=count; return out;
    }
    uint64_t number(size_t width) { const auto result=integer(bytes,cursor,width); cursor+=width; return result; }
    Bytes chunk(const std::string& tag,size_t maximum) {
        const auto name=take(4);
        if (std::string(name.begin(),name.end())!=tag || number(4)!=1) throw std::runtime_error("native chunk/version");
        const auto length=number(8); if (length>maximum) throw std::runtime_error("native chunk budget");
        const auto expected=hex(take(32)); const auto data=take(static_cast<size_t>(length));
        if (fallout_tables::hash(data)!=expected) throw std::runtime_error("native chunk checksum"); return data;
    }
};
int wmain(int argc,wchar_t** argv) {
    try {
        const bool legacy2=argc==3 && std::wstring(argv[2])==L"--schema-2";
        const bool legacy3=argc==3 && std::wstring(argv[2])==L"--schema-3";
        if (argc!=2 && !legacy2 && !legacy3) throw std::runtime_error("usage: native-save-oracle native_container.frsv [--schema-2|--schema-3]");
        const auto state_schema=legacy2?2:(legacy3?3:4);
        NativeFile source(argv[1]); const auto& bytes=source.bytes;
        const Bytes unsigned_bytes(bytes.begin(),bytes.end()-32), checksum(bytes.end()-32,bytes.end());
        if (fallout_tables::hash(unsigned_bytes)!=hex(checksum)) throw std::runtime_error("native whole checksum");
        Reader reader{unsigned_bytes}; const auto magic=reader.take(8);
        if (std::string(magic.begin(),magic.end())!="FRSAVE01" || reader.number(2)!=1 || reader.number(2)!=0 || reader.number(4)!=2) throw std::runtime_error("native header/version");
        const auto metadata=reader.chunk("META",88); if (metadata.size()!=88) throw std::runtime_error("native metadata extent");
        Reader meta{metadata};
        if (meta.number(4)!=1 || meta.number(4)!=state_schema) throw std::runtime_error("native profile/state schema");
        const auto generation=meta.number(8); if (!generation) throw std::runtime_error("native generation zero");
        const auto tick=meta.number(8); const auto cohort=hex(meta.take(32)); const auto snapshot_size=meta.number(8);
        const auto campaign=meta.take(16); const auto revision=meta.number(8);
        if (std::all_of(campaign.begin(),campaign.end(),[](auto byte){return byte==0;})) throw std::runtime_error("native campaign zero");
        const auto state=reader.chunk("STAT",64*1024*1024);
        if (state.size()!=snapshot_size || reader.cursor!=unsigned_bytes.size()) throw std::runtime_error("native state/trailing extent");
        std::vector<std::string> identity; for (const auto byte:campaign) identity.push_back(std::to_string(byte));
        const auto report=object({{"campaign",array(identity)},{"state_revision",std::to_string(revision)},
            {"generation",std::to_string(generation)},{"boundary_tick",std::to_string(tick)},{"catalogue_sha256",quote(cohort)},
            {"snapshot_bytes",std::to_string(state.size())},{"snapshot_sha256",quote(fallout_tables::hash(state))},
            {"container_bytes",std::to_string(bytes.size())},{"container_sha256",quote(fallout_tables::hash(bytes))}});
        std::cout<<object({{"schema_version","1"},{"state_schema",std::to_string(state_schema)},{"metadata",report},
            {"scope",quote("Independent native container extents, metadata and checksums; canonical JSON semantics not evaluated")},
            {"retail_save_compatibility","false"}})<<'\n';
        return 0;
    } catch(const std::exception& error) { std::cerr<<"native-save-oracle: "<<error.what()<<'\n'; return 1; }
}
