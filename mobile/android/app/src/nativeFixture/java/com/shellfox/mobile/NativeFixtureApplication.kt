package com.shellfox.mobile

import android.app.Application

/** JNI fixture process only. Never initializes React Native, tsnet or an account. */
class NativeFixtureApplication : Application()
