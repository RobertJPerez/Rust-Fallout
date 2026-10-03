#include "body.hpp"
using namespace fallout_records;
static void require(bool result,const char* message){if(!result)throw std::runtime_error(message);}
template<class Action>static void rejects(Action action,const std::string& diagnostic){
    try{action();}catch(const std::runtime_error& error){require(std::string(error.what()).find(diagnostic)!=std::string::npos,"wrong rejection point");return;}
    throw std::runtime_error("hostile body was accepted");
}
int main(){
    try{
        unsigned checked=0;
        for(const auto used_start:std::vector<uint64_t>{0,actor_body::catalogue_limit-1024}){
            bool read=false,inflate=false;uint64_t used=used_start;
            const auto maximum=static_cast<size_t>(std::min(actor_body::record_limit,actor_body::catalogue_limit-used));
            rejects([&]{actor_body::bounded(maximum+1,0,used,[&](size_t){read=true;return Bytes{};},[&](const Bytes&,size_t,size_t)->fallout_zlib::Decoded{inflate=true;throw std::runtime_error("unexpected inflate");});},"before read");
            require(!read&&!inflate&&used==used_start,"stored-size rejection allocated/read or changed ledger");++checked;
        }
        for(const auto used_start:std::vector<uint64_t>{0,actor_body::catalogue_limit-1024}){
            bool read=false,inflate=false;uint64_t used=used_start;
            const auto maximum=static_cast<size_t>(std::min(actor_body::record_limit,actor_body::catalogue_limit-used));
            Bytes prefix;append(prefix,maximum+1,4);prefix.push_back(0);
            rejects([&]{actor_body::bounded(prefix.size(),0x40000,used,[&](size_t size){read=true;require(size==prefix.size(),"wrong read extent");return prefix;},[&](const Bytes&,size_t,size_t)->fallout_zlib::Decoded{inflate=true;throw std::runtime_error("unexpected inflate");});},"before inflate");
            require(read&&!inflate&&used==used_start,"declared-size rejection inflated or changed ledger");++checked;
        }
        {
            uint64_t used=actor_body::catalogue_limit-4;bool inflate=false;
            const auto result=actor_body::bounded(4,0,used,[](size_t){return Bytes{1,2,3,4};},[&](const Bytes&,size_t,size_t)->fallout_zlib::Decoded{inflate=true;throw std::runtime_error("unexpected inflate");});
            require(result==Bytes({1,2,3,4})&&used==actor_body::catalogue_limit&&!inflate,"exact remaining uncompressed admission");++checked;
        }
        {
            // Independently authored zlib stream for one zero byte.
            Bytes stored{1,0,0,0,0x78,0x9c,0x63,0,0,0,1,0,1};uint64_t used=0;
            const auto result=actor_body::bounded(stored.size(),0x40000,used,[&](size_t){return stored;},[](const Bytes& input,size_t expected,size_t maximum){return fallout_zlib::decode(input,expected,maximum);});
            require(result==Bytes({0})&&used==1,"valid compressed body changed");++checked;
        }
        std::cout<<"actor body allocation-order checks: "<<checked<<" passed\n";return 0;
    }catch(const std::exception& error){std::cerr<<"actor body test: "<<error.what()<<'\n';return 1;}
}
