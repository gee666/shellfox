#include "jni_status.h"
#include "terminal_frame.h"
#include <limits>

namespace {
void fail(JNIEnv* env, const char* message) {
  if (!env->ExceptionCheck()) env->ThrowNew(env->FindClass("java/lang/IllegalStateException"), message);
}
bool status(JNIEnv* env, SfmStatus result) {
  if (result == SFM_OK) return true;
  throwStatus(env, result);
  return false;
}
jbyteArray bytes(JNIEnv* env, const std::vector<uint8_t>& value) {
  if (value.size() > static_cast<size_t>(std::numeric_limits<jsize>::max())) {
    fail(env, "Native response too large");
    return nullptr;
  }
  auto out = env->NewByteArray(static_cast<jsize>(value.size()));
  if (out && !value.empty()) env->SetByteArrayRegion(out, 0, static_cast<jsize>(value.size()),
      reinterpret_cast<const jbyte*>(value.data()));
  return out;
}
struct Copy {
  std::vector<uint8_t> data;
  bool failed = false;
  size_t limit = 16 * 1024 * 1024;
  uint8_t user_hint = 0;
  bool has_input_hint = false;
};
void frame(void* context, const SfmFrame* value) noexcept {
  auto& out = *static_cast<Copy*>(context);
  try {
    if (!value) { out.failed = true; return; }
    out.data = shellfox::copyFrame(*value);
  } catch (...) { out.failed = true; }
}
void encoded(void* context, const uint8_t* data, size_t size) noexcept {
  auto& out = *static_cast<Copy*>(context);
  try {
    if (size > out.limit || (size && !data) ||
        out.data.size() + size > out.limit) { out.failed = true; return; }
    if (size) out.data.insert(out.data.end(), data, data + size);
  } catch (...) { out.failed = true; }
}
void encodedInput(void* context, const uint8_t* data, size_t size, uint8_t user_hint) noexcept {
  auto& out = *static_cast<Copy*>(context);
  if (user_hint > 1 || size == 0 || (out.has_input_hint && out.user_hint != user_hint)) {
    out.failed = true;
    return;
  }
  encoded(context, data, size);
  out.user_hint = user_hint;
  out.has_input_hint = true;
}
bool inputBytes(JNIEnv* env, jbyteArray input, size_t limit, std::vector<uint8_t>& out) {
  if (!input) { fail(env, "Missing native input"); return false; }
  const auto size = env->GetArrayLength(input);
  if (static_cast<size_t>(size) > limit) { fail(env, "Native input too large"); return false; }
  out.resize(size);
  env->GetByteArrayRegion(input, 0, size, reinterpret_cast<jbyte*>(out.data()));
  return !env->ExceptionCheck();
}
}
#define JNI(name) Java_com_shellfox_mobile_rust_RustBindings_##name
extern "C" JNIEXPORT jint JNICALL JNI(abiVersion)(JNIEnv*, jobject) { return sfm_abi_version(); }
extern "C" JNIEXPORT void JNICALL JNI(reset)(JNIEnv* env, jobject, jlong handle, jint cols, jint rows) {
  if (cols < 1 || rows < 1 || cols > 512 || rows > 512) { fail(env, "Invalid grid"); return; }
  status(env, sfm_terminal_reset(static_cast<SfmHandle>(handle), cols, rows));
}
extern "C" JNIEXPORT void JNICALL JNI(scroll)(JNIEnv* env, jobject, jlong handle, jint rows) {
  status(env, sfm_terminal_scroll(static_cast<SfmHandle>(handle), rows));
}
extern "C" JNIEXPORT void JNICALL JNI(select)(JNIEnv* env, jobject, jlong handle,
    jint kind, jint sx, jint sy, jint ex, jint ey) {
  if (kind < 0 || kind > 4 || sx < 0 || sy < 0 || ex < 0 || ey < 0 || sx > 511 || sy > 511 || ex > 511 || ey > 511) {
    fail(env, "Invalid selection"); return;
  }
  const SfmSelection selection{static_cast<uint32_t>(kind), static_cast<uint16_t>(sx),
    static_cast<uint16_t>(sy), static_cast<uint16_t>(ex), static_cast<uint16_t>(ey), 0};
  status(env, sfm_terminal_select(static_cast<SfmHandle>(handle), selection));
}
extern "C" JNIEXPORT jbyteArray JNICALL JNI(copy)(JNIEnv* env, jobject, jlong handle) {
  Copy out;
  if (!status(env, sfm_terminal_copy(static_cast<SfmHandle>(handle), encoded, &out))) return nullptr;
  if (out.failed) { fail(env, "Native selection too large"); return nullptr; }
  return bytes(env, out.data);
}
static jbyteArray copyLink(JNIEnv* env, jlong handle, jint x, jint y, decltype(&sfm_terminal_hyperlink) read) {
  if (x < 0 || y < 0 || x > 511 || y > 511) { fail(env, "Invalid link position"); return nullptr; }
  Copy out;
  if (!status(env, read(static_cast<SfmHandle>(handle), x, y, encoded, &out))) return nullptr;
  if (out.failed) { fail(env, "Native link too large"); return nullptr; }
  return bytes(env, out.data);
}
extern "C" JNIEXPORT jbyteArray JNICALL JNI(hyperlink)(JNIEnv* env, jobject, jlong handle, jint x, jint y) {
  return copyLink(env, handle, x, y, sfm_terminal_hyperlink);
}
extern "C" JNIEXPORT jbyteArray JNICALL JNI(fileLink)(JNIEnv* env, jobject, jlong handle, jint x, jint y) {
  return copyLink(env, handle, x, y, sfm_terminal_file_link);
}
extern "C" JNIEXPORT jboolean JNICALL JNI(mouseTracking)(JNIEnv* env, jobject, jlong handle) {
  uint8_t enabled = 0;
  return status(env, sfm_terminal_mouse_tracking(static_cast<SfmHandle>(handle), &enabled)) && enabled;
}
extern "C" JNIEXPORT jlong JNICALL JNI(create)(JNIEnv* env, jobject, jint cols, jint rows) {
  if (sfm_abi_version() != SFM_ABI_VERSION || cols < 1 || rows < 1 || cols > 512 || rows > 512) {
    fail(env, "Unsupported terminal configuration"); return 0;
  }
  SfmHandle handle = 0;
  if (!status(env, sfm_terminal_create(cols, rows, &handle))) return 0;
  return static_cast<jlong>(handle);
}
extern "C" JNIEXPORT void JNICALL JNI(close)(JNIEnv* env, jobject, jlong handle) {
  status(env, sfm_terminal_close(static_cast<SfmHandle>(handle)));
}
extern "C" JNIEXPORT jint JNICALL JNI(submit)(JNIEnv* env, jobject, jlong handle, jlong sequence, jbyteArray data) {
  try {
    std::vector<uint8_t> value;
    if (sequence == 0 || !inputBytes(env, data, SFM_MAX_CHUNK_BYTES, value)) return SFM_INVALID;
    return sfm_terminal_submit(static_cast<SfmHandle>(handle), sequence, value.data(), value.size());
  } catch (...) { fail(env, "Native allocation failed"); return SFM_ENGINE; }
}
extern "C" JNIEXPORT jlongArray JNICALL JNI(progress)(JNIEnv* env, jobject, jlong handle) {
  SfmProgress progress{};
  if (!status(env, sfm_terminal_progress(static_cast<SfmHandle>(handle), &progress))) return nullptr;
  const jlong values[] = {static_cast<jlong>(progress.accepted_sequence),
    static_cast<jlong>(progress.consumed_sequence), progress.queued_bytes, progress.queued_chunks};
  auto out = env->NewLongArray(4);
  if (out) env->SetLongArrayRegion(out, 0, 4, values);
  return out;
}
extern "C" JNIEXPORT void JNICALL JNI(pump)(JNIEnv* env, jobject, jlong handle) {
  status(env, sfm_terminal_pump(static_cast<SfmHandle>(handle), nullptr, nullptr));
}
extern "C" JNIEXPORT jbyteArray JNICALL JNI(pumpFrame)(JNIEnv* env, jobject, jlong handle) {
  if (!status(env, sfm_terminal_pump(static_cast<SfmHandle>(handle), nullptr, nullptr))) return nullptr;
  Copy out;
  if (!status(env, sfm_terminal_frame(static_cast<SfmHandle>(handle), frame, &out))) return nullptr;
  if (out.failed) { fail(env, "Native frame copy failed"); return nullptr; }
  return bytes(env, out.data);
}
extern "C" JNIEXPORT void JNICALL JNI(resize)(JNIEnv* env, jobject, jlong handle, jint cols, jint rows) {
  if (cols < 1 || rows < 1 || cols > 512 || rows > 512) {
    fail(env, "Invalid terminal dimensions"); return;
  }
  status(env, sfm_terminal_resize(static_cast<SfmHandle>(handle), cols, rows));
}
extern "C" JNIEXPORT void JNICALL JNI(configure)(JNIEnv* env, jobject, jlong handle, jbyteArray json) {
  try {
    std::vector<uint8_t> value;
    if (inputBytes(env, json, 16384, value))
      status(env, sfm_terminal_configure(static_cast<SfmHandle>(handle), value.data(), value.size()));
  } catch (...) { fail(env, "Native allocation failed"); }
}
extern "C" JNIEXPORT jbyteArray JNICALL JNI(inputEx)(JNIEnv* env, jobject, jlong handle, jbyteArray json) {
  try {
    std::vector<uint8_t> value;
    if (!inputBytes(env, json, SFM_MAX_INPUT_BYTES, value)) return nullptr;
    Copy out;
    out.limit = SFM_MAX_INPUT_BYTES;
    if (!status(env, sfm_terminal_input_ex(handle, value.data(), value.size(), encodedInput, &out))) return nullptr;
    if (out.failed) { fail(env, "Native input copy failed"); return nullptr; }
    // Private JNI packet: the Rust hint byte followed by length-delimited input.
    if (out.has_input_hint) out.data.insert(out.data.begin(), out.user_hint);
    return bytes(env, out.data);
  } catch (...) { fail(env, "Native allocation failed"); return nullptr; }
}
extern "C" JNIEXPORT jbyteArray JNICALL JNI(input)(JNIEnv* env, jobject, jlong handle, jbyteArray json) {
  try {
    std::vector<uint8_t> value;
    if (!inputBytes(env, json, SFM_MAX_INPUT_BYTES, value)) return nullptr;
    Copy out;
    out.limit = SFM_MAX_INPUT_BYTES;
    if (!status(env, sfm_terminal_input(static_cast<SfmHandle>(handle), value.data(), value.size(), encoded, &out)))
      return nullptr;
    if (out.failed) { fail(env, "Native input copy failed"); return nullptr; }
    return bytes(env, out.data);
  } catch (...) { fail(env, "Native allocation failed"); return nullptr; }
}
