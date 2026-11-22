#include "jni_status.h"
#include "shellfox_mobile_client.h"
#include "client_profile_ops.h"
#include "client_jni_bytes.h"
#include <mutex>
#include <vector>

namespace {
void fail(JNIEnv* env, SfmStatus result) {
  throwStatus(env, result);
}
bool check(JNIEnv* env, SfmStatus result) { if (result == SFM_OK) return true; fail(env, result); return false; }
std::vector<uint8_t> copy(JNIEnv* env, jbyteArray source, size_t limit) {
  return copyClientBytes(env,source,limit);
}
jbyteArray array(JNIEnv* env, const uint8_t* bytes, size_t length) {
  if (length > SFM_CLIENT_MAX_RESPONSE_BYTES) { fail(env, SFM_INVALID); return nullptr; }
  auto out = env->NewByteArray(length);
  if (out && length) env->SetByteArrayRegion(out, 0, length, reinterpret_cast<const jbyte*>(bytes));
  return out;
}
uint32_t clientProfile(JNIEnv* env, SfmClientHandle client) {
  uint32_t profile = 0, capabilities = 0;
  if (!check(env, sfm_client_profile(client, &profile, &capabilities))) return 0;
  if (!client_ops::forProfile(profile)) {
    fail(env, SFM_INVALID); return 0;
  }
  return profile;
}
struct Dispatch { JNIEnv* env; jclass transport; uint32_t profile = SFM_CLIENT_PROFILE_V1; };
void request(void* context, SfmClientHandle client, uint64_t generation, uint64_t id,
    const uint8_t* peer, size_t peerLen, const uint8_t* path, size_t pathLen,
    const uint8_t* body, size_t bodyLen, uint32_t deadline, size_t limit) noexcept {
  auto& ctx = *static_cast<Dispatch*>(context);
  auto* env = ctx.env;
  // Locals and callback context live only for this synchronous Rust dispatch call.
  auto method = env->GetStaticMethodID(ctx.transport,
    client_ops::statusTransport(ctx.profile) ? "requestHttp" : "request", "(JJJ[B[B[BIJ)V");
  auto p = array(env, peer, peerLen);
  auto route = array(env, path, pathLen);
  auto data = array(env, body, bodyLen);
  if (method && p && route && data && !env->ExceptionCheck())
    env->CallStaticVoidMethod(ctx.transport, method, static_cast<jlong>(client), static_cast<jlong>(generation),
      static_cast<jlong>(id), p, route, data, static_cast<jint>(deadline), static_cast<jlong>(limit));
  if (env->ExceptionCheck()) {
    env->ExceptionClear();
    if (client_ops::statusTransport(ctx.profile))
      client_ops::completeHttp(ctx.profile,client, generation, id, SFM_TRANSPORT_UNAVAILABLE, 0, nullptr, 0);
    else sfm_client_complete(client, generation, id, SFM_TRANSPORT_UNAVAILABLE, nullptr, 0);
  }
  if (p) env->DeleteLocalRef(p);
  if (route) env->DeleteLocalRef(route);
  if (data) env->DeleteLocalRef(data);
}
// Set once on a JVM thread; Rust wakes from its own threads, which attach as daemons.
JavaVM* wakeVm = nullptr;
jclass wakeClass = nullptr;
jmethodID wakeMethod = nullptr;
void wake(void*, SfmClientHandle client) noexcept {
  JNIEnv* env = nullptr;
  if (wakeVm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6) != JNI_OK &&
    wakeVm->AttachCurrentThreadAsDaemon(&env, nullptr) != JNI_OK) return;
  env->CallStaticVoidMethod(wakeClass, wakeMethod, static_cast<jlong>(client));
  if (env->ExceptionCheck()) env->ExceptionClear();
}
void cancel(void* context, SfmClientHandle client, uint64_t generation, uint64_t id) noexcept {
  auto& ctx = *static_cast<Dispatch*>(context);
  auto method = ctx.env->GetStaticMethodID(ctx.transport, "cancel", "(JJJ)V");
  if (method) ctx.env->CallStaticVoidMethod(ctx.transport, method, static_cast<jlong>(client),
    static_cast<jlong>(generation), static_cast<jlong>(id));
  if (ctx.env->ExceptionCheck()) ctx.env->ExceptionClear();
}
}
#define JNI(name) Java_com_shellfox_mobile_rust_ClientBindings_##name
extern "C" JNIEXPORT jlong JNICALL JNI(create)(JNIEnv* env, jobject, jbyteArray peer, jbyteArray id) {
  try {
    auto p = copy(env, peer, 256);
    auto i = copy(env, id, 64);
    if (env->ExceptionCheck()) return 0;
    SfmClientHandle handle = 0;
    if (sfm_client_abi_version() != SFM_CLIENT_ABI_VERSION) { fail(env, SFM_INVALID); return 0; }
    check(env, sfm_client_create(p.data(), p.size(), i.data(), i.size(), &handle));
    return static_cast<jlong>(handle);
  } catch (...) { fail(env, SFM_ENGINE); return 0; }
}
extern "C" JNIEXPORT jint JNICALL JNI(observerAbiVersion)(JNIEnv*, jobject) {
  return sfm_observer_v2_abi_version();
}
extern "C" JNIEXPORT jlong JNICALL JNI(createObserver)(JNIEnv* env, jobject, jbyteArray peer, jbyteArray id) {
  try {
    auto p = copy(env, peer, 256);
    auto i = copy(env, id, 64);
    if (env->ExceptionCheck()) return 0;
    if (sfm_observer_v2_abi_version() != SFM_OBSERVER_V2_ABI_VERSION) { fail(env, SFM_INVALID); return 0; }
    SfmClientHandle handle = 0;
    check(env, sfm_observer_v2_create(p.data(), p.size(), i.data(), i.size(), &handle));
    return static_cast<jlong>(handle);
  } catch (...) { fail(env, SFM_ENGINE); return 0; }
}
extern "C" JNIEXPORT jlong JNICALL JNI(discoverObserver)(JNIEnv* env, jobject, jlong handle, jboolean offer_legacy) {
  uint64_t operation = 0;
  if (offer_legacy > 1) { fail(env, SFM_INVALID); return 0; }
  check(env, sfm_observer_v2_discover(handle, offer_legacy, &operation));
  return static_cast<jlong>(operation);
}
extern "C" JNIEXPORT jlong JNICALL JNI(pairObserver)(JNIEnv* env, jobject, jlong handle, jbyteArray attempt) {
  try {
    auto data = copy(env, attempt, 64);
    if (env->ExceptionCheck()) return 0;
    uint64_t operation = 0;
    check(env, sfm_observer_v2_pair(handle, data.data(), data.size(), &operation));
    return static_cast<jlong>(operation);
  } catch (...) { fail(env, SFM_ENGINE); return 0; }
}
extern "C" JNIEXPORT void JNICALL JNI(reconnectObserver)(JNIEnv* env, jobject, jlong handle, jbyteArray id) {
  try {
    auto data = copy(env, id, 64);
    if (!env->ExceptionCheck()) check(env, sfm_observer_v2_reconnect(handle, data.data(), data.size()));
  } catch (...) { fail(env, SFM_ENGINE); }
}
extern "C" JNIEXPORT jintArray JNICALL JNI(profile)(JNIEnv* env, jobject, jlong handle) {
  uint32_t profile = 0, capabilities = 0;
  if (!check(env, sfm_client_profile(handle, &profile, &capabilities))) return nullptr;
  const jint values[] = {static_cast<jint>(profile), static_cast<jint>(capabilities)};
  auto out = env->NewIntArray(2);
  if (out) env->SetIntArrayRegion(out, 0, 2, values);
  return out;
}
extern "C" JNIEXPORT jlong JNICALL JNI(start)(JNIEnv* env, jobject, jlong handle, jint kind, jbyteArray json) {
  try {
    const auto profile = clientProfile(env, handle);
    if (env->ExceptionCheck()) return 0;
    const auto* api=client_ops::forProfile(profile);
    uint64_t operation = 0;
    SfmStatus result = SFM_INVALID;
    switch (kind) {
      case 0: if (profile==SFM_CLIENT_PROFILE_V1) result = sfm_client_connect(handle, &operation); break;
      case 1: if (profile==SFM_CLIENT_PROFILE_V1) result = sfm_client_reconnect(handle, &operation); break;
      case 2: result = api->poll(handle, &operation); break;
      case 4: result = api->disconnect(handle, &operation); break;
      case 3: {
        auto data = copy(env, json, SFM_CLIENT_MAX_REQUEST_BYTES);
        if (env->ExceptionCheck()) return 0;
        result = api->operation(handle, data.data(), data.size(), &operation); break;
      }
    }
    check(env, result);
    return static_cast<jlong>(operation);
  } catch (...) { fail(env, SFM_ENGINE); return 0; }
}
extern "C" JNIEXPORT void JNICALL JNI(cancelOperation)(JNIEnv* env, jobject, jlong handle, jlong operation) {
  check(env, sfm_client_cancel(handle, operation));
}
extern "C" JNIEXPORT void JNICALL JNI(bind)(JNIEnv* env, jobject, jlong handle, jbyteArray tab, jlong terminal) {
  try {
    auto data = copy(env, tab, 64);
    const auto profile = clientProfile(env, handle);
    if (!env->ExceptionCheck()) check(env,client_ops::forProfile(profile)->bind(handle,data.data(),data.size(),terminal));
  } catch (...) { fail(env, SFM_ENGINE); }
}
extern "C" JNIEXPORT void JNICALL JNI(unbind)(JNIEnv* env, jobject, jlong handle, jlong terminal) {
  const auto profile = clientProfile(env, handle);
  if (!env->ExceptionCheck()) check(env,client_ops::forProfile(profile)->unbind(handle,terminal));
}
extern "C" JNIEXPORT jint JNICALL JNI(consumed)(JNIEnv* env, jobject, jlong handle, jlong terminal) {
  const auto profile = clientProfile(env, handle);
  if (env->ExceptionCheck()) return SFM_INVALID;
  return client_ops::forProfile(profile)->consumed(handle,terminal);
}
extern "C" JNIEXPORT jlong JNICALL JNI(resetTicket)(JNIEnv* env, jobject, jlong handle, jlong terminal) {
  uint64_t ticket = 0;
  const auto profile = clientProfile(env, handle);
  if (env->ExceptionCheck()) return 0;
  const auto* api=client_ops::forProfile(profile);
  if(!api->ticket)return 0;
  const auto result=api->ticket(handle,terminal,&ticket);
  if (result == SFM_CLIENT_EMPTY) return 0;
  check(env, result);
  return static_cast<jlong>(ticket);
}
extern "C" JNIEXPORT jintArray JNICALL JNI(resetGrid)(JNIEnv* env, jobject, jlong handle, jlong terminal, jlong ticket) {
  uint16_t cols = 0, rows = 0;
  const auto profile = clientProfile(env, handle);
  if (env->ExceptionCheck()) return nullptr;
  const auto* api=client_ops::forProfile(profile);
  if(!api->grid){fail(env,SFM_INVALID);return nullptr;}
  const auto result=api->grid(handle,terminal,ticket,&cols,&rows);
  const jint values[] = {result, cols, rows};
  auto out = env->NewIntArray(3);
  if (out) env->SetIntArrayRegion(out, 0, 3, values);
  return out;
}
extern "C" JNIEXPORT jint JNICALL JNI(resetDone)(JNIEnv* env, jobject, jlong handle, jlong terminal, jlong ticket) {
  const auto profile = clientProfile(env, handle);
  if (env->ExceptionCheck()) return SFM_INVALID;
  const auto* api=client_ops::forProfile(profile);
  return api->reset?api->reset(handle,terminal,ticket):SFM_INVALID;
}
extern "C" JNIEXPORT jboolean JNICALL JNI(recoveryReady)(JNIEnv* env, jobject, jlong handle, jlong terminal) {
  uint8_t ready = 0;
  const auto profile = clientProfile(env, handle);
  if (env->ExceptionCheck()) return false;
  return check(env,client_ops::forProfile(profile)->ready(handle,terminal,&ready))&&ready;
}
extern "C" JNIEXPORT jint JNICALL JNI(resize)(JNIEnv* env, jobject, jlong handle, jlong terminal, jint cols, jint rows) {
  const auto profile = clientProfile(env, handle);
  if (env->ExceptionCheck()) return SFM_INVALID;
  if (profile == SFM_CLIENT_PROFILE_OBSERVER_V2) return SFM_CLIENT_PROTOCOL;
  if (cols < 2 || rows < 2 || cols > 500 || rows > 500) return SFM_INVALID;
  uint64_t operation = 0;
  return client_ops::resize(profile,handle,terminal,cols,rows,&operation);
}
extern "C" JNIEXPORT jint JNICALL JNI(write)(JNIEnv* env, jobject, jlong handle, jlong terminal, jbyteArray input, jboolean user_initiated) {
  try {
    auto data = copy(env, input, SFM_MAX_INPUT_BYTES);
    if (env->ExceptionCheck()) return 0;
    const auto profile = clientProfile(env, handle);
    if (env->ExceptionCheck()) return SFM_INVALID;
    if (profile != SFM_CLIENT_PROFILE_V1) return SFM_CLIENT_PROTOCOL;
    uint64_t operation = 0;
    if (user_initiated > 1) return SFM_INVALID;
    return sfm_client_terminal_write_ex(handle, terminal, data.data(), data.size(), user_initiated, &operation);
  } catch (...) { fail(env, SFM_ENGINE); return SFM_ENGINE; }
}
extern "C" JNIEXPORT void JNICALL JNI(dispatch)(JNIEnv* env, jobject, jlong handle, jclass transport) {
  const auto profile = clientProfile(env, handle);
  if (env->ExceptionCheck()) return;
  Dispatch ctx{env, transport, profile};
  check(env, sfm_client_dispatch(handle, request, cancel, &ctx));
}
extern "C" JNIEXPORT void JNICALL JNI(close)(JNIEnv* env, jobject, jlong handle, jclass transport) {
  Dispatch ctx{env, transport};
  check(env, sfm_client_close(handle, cancel, &ctx));
}
extern "C" JNIEXPORT jint JNICALL JNI(complete)(JNIEnv* env, jobject, jlong handle, jlong generation,
    jlong id, jint outcome, jbyteArray response) {
  try {
    auto data = copy(env, response, SFM_CLIENT_MAX_RESPONSE_BYTES);
    if (env->ExceptionCheck()) return SFM_INVALID;
    return sfm_client_complete(handle, generation, id, outcome, data.data(), data.size());
  } catch (...) { return SFM_ENGINE; }
}
extern "C" JNIEXPORT jint JNICALL JNI(completeHttp)(JNIEnv* env, jobject, jlong handle, jlong generation,
    jlong id, jint outcome, jint http_status, jbyteArray response) {
  try {
    auto data = copy(env, response, SFM_CLIENT_MAX_RESPONSE_BYTES);
    if (env->ExceptionCheck()) return SFM_INVALID;
    if (http_status < 0 || http_status > 599) return SFM_INVALID;
    const auto profile=clientProfile(env,handle);
    if(env->ExceptionCheck())return SFM_INVALID;
    return client_ops::completeHttp(profile,handle, generation, id, outcome, static_cast<uint16_t>(http_status),data.data(),data.size());
  } catch (...) { return SFM_ENGINE; }
}
extern "C" JNIEXPORT jbyteArray JNICALL JNI(readEvent)(JNIEnv* env, jobject, jlong handle) {
  try {
    std::vector<uint8_t> data(4096);
    size_t needed = 0;
    auto result = sfm_client_read_event(handle, data.data(), data.size(), &needed);
    if (result == SFM_CLIENT_EMPTY) return nullptr;
    if (result == SFM_CLIENT_BUFFER && needed <= SFM_CLIENT_MAX_RESPONSE_BYTES) {
      data.resize(needed);
      result = sfm_client_read_event(handle, data.data(), data.size(), &needed);
    }
    if (!check(env, result) || needed > data.size()) return nullptr;
    return array(env, data.data(), needed);
  } catch (...) { fail(env, SFM_ENGINE); return nullptr; }
}
extern "C" JNIEXPORT void JNICALL JNI(setWaker)(JNIEnv* env, jobject bindings, jlong handle) {
  static std::once_flag once;
  std::call_once(once, [&] {
    if (env->GetJavaVM(&wakeVm) != JNI_OK) return;
    auto local = env->GetObjectClass(bindings);
    wakeClass = static_cast<jclass>(env->NewGlobalRef(local));
    env->DeleteLocalRef(local);
    wakeMethod = env->GetStaticMethodID(wakeClass, "wake", "(J)V");
  });
  if (env->ExceptionCheck()) return;
  if (!wakeVm || !wakeClass || !wakeMethod) { fail(env, SFM_ENGINE); return; }
  check(env, sfm_client_set_waker(handle, wake, nullptr));
}
