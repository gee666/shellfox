#include "client_jni_bytes.h"
#include "shellfox_mobile_client.h"
#define JNI(name) Java_com_shellfox_mobile_rust_ClientBindings_##name
namespace {
#ifdef SFM_ENABLE_MANAGER_V2
void checked(JNIEnv* env,SfmStatus status){if(status!=SFM_OK)throwStatus(env,status);}
#endif
}
extern "C" JNIEXPORT jint JNICALL JNI(managerAbiVersion)(JNIEnv*,jobject){
#ifdef SFM_ENABLE_MANAGER_V2
  return sfm_manager_v2_abi_version();
#else
  return 0;
#endif
}
extern "C" JNIEXPORT jlong JNICALL JNI(createManager)(JNIEnv* env,jobject,jbyteArray peer,jbyteArray identity){
#ifdef SFM_ENABLE_MANAGER_V2
  try{auto p=copyClientBytes(env,peer,256),id=copyClientBytes(env,identity,36);if(env->ExceptionCheck())return 0;SfmClientHandle handle=0;checked(env,sfm_manager_v2_create(p.data(),p.size(),id.data(),id.size(),&handle));return static_cast<jlong>(handle);}catch(...){throwStatus(env,SFM_ENGINE);return 0;}
#else
  (void)peer;(void)identity;throwStatus(env,SFM_CLIENT_PROTOCOL);return 0;
#endif
}
extern "C" JNIEXPORT jlong JNICALL JNI(discoverManager)(JNIEnv* env,jobject,jlong handle){
#ifdef SFM_ENABLE_MANAGER_V2
  uint64_t operation=0;checked(env,sfm_manager_v2_discover(handle,&operation));return static_cast<jlong>(operation);
#else
  (void)handle;throwStatus(env,SFM_CLIENT_PROTOCOL);return 0;
#endif
}
extern "C" JNIEXPORT jlong JNICALL JNI(pairManager)(JNIEnv* env,jobject,jlong handle,jbyteArray attempt){
#ifdef SFM_ENABLE_MANAGER_V2
  try{auto id=copyClientBytes(env,attempt,36);if(env->ExceptionCheck())return 0;uint64_t operation=0;checked(env,sfm_manager_v2_pair(handle,id.data(),id.size(),&operation));return static_cast<jlong>(operation);}catch(...){throwStatus(env,SFM_ENGINE);return 0;}
#else
  (void)handle;(void)attempt;throwStatus(env,SFM_CLIENT_PROTOCOL);return 0;
#endif
}
extern "C" JNIEXPORT void JNICALL JNI(reconnectManager)(JNIEnv* env,jobject,jlong handle,jbyteArray identity){
#ifdef SFM_ENABLE_MANAGER_V2
  try{auto id=copyClientBytes(env,identity,36);if(!env->ExceptionCheck())checked(env,sfm_manager_v2_reconnect(handle,id.data(),id.size()));}catch(...){throwStatus(env,SFM_ENGINE);}
#else
  (void)handle;(void)identity;throwStatus(env,SFM_CLIENT_PROTOCOL);
#endif
}
extern "C" JNIEXPORT jint JNICALL JNI(inputManager)(JNIEnv* env,jobject,jlong handle,jlong terminal,jbyteArray json,jboolean hint){
#ifdef SFM_ENABLE_MANAGER_V2
  try{auto input=copyClientBytes(env,json,SFM_CLIENT_MAX_REQUEST_BYTES);if(env->ExceptionCheck())return SFM_INVALID;if(hint>1)return SFM_INVALID;uint64_t operation=0;return sfm_manager_v2_terminal_input(handle,terminal,input.data(),input.size(),hint,&operation);}catch(...){throwStatus(env,SFM_ENGINE);return SFM_ENGINE;}
#else
  (void)handle;(void)terminal;(void)json;(void)hint;throwStatus(env,SFM_CLIENT_PROTOCOL);return SFM_CLIENT_PROTOCOL;
#endif
}
