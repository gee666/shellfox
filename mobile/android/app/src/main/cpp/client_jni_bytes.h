#pragma once
#include "jni_status.h"
#include "shellfox_mobile.h"
#include <vector>
inline std::vector<uint8_t> copyClientBytes(JNIEnv* env,jbyteArray source,size_t limit){
  if(!source||static_cast<size_t>(env->GetArrayLength(source))>limit){throwStatus(env,SFM_INVALID);return {};}
  std::vector<uint8_t> result(env->GetArrayLength(source));
  env->GetByteArrayRegion(source,0,result.size(),reinterpret_cast<jbyte*>(result.data()));
  return result;
}
