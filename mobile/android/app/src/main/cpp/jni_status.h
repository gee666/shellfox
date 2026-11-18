#pragma once
#include <jni.h>

// Carry stable ABI status codes, never native diagnostics or transport bodies.
inline void throwStatus(JNIEnv* env, jint status) {
  if (env->ExceptionCheck()) return;
  auto type = env->FindClass("com/shellfox/mobile/rust/NativeCallException");
  if (!type) return;
  auto constructor = env->GetMethodID(type, "<init>", "(I)V");
  if (constructor) {
    auto error = env->NewObject(type, constructor, status);
    if (error) { env->Throw(static_cast<jthrowable>(error)); env->DeleteLocalRef(error); }
  }
  env->DeleteLocalRef(type);
}
