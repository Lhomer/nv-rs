//! What the Android app tells the player when it cannot start the game (docs/ANDROID.md;
//! Patrick, 2026-11-16, a black screen on the Galaxy Z TriFold): the "All files access" check and
//! its settings page, and a message drawn in the activity's window (Android's own fonts, drawn by
//! `android.graphics.Canvas` into a bitmap, copied into the window) instead of a black screen.
//!
//! All through JNI from the app's native thread: framework classes only (`android.os.Environment`,
//! `android.content.Intent`, `android.net.Uri`, `android.graphics.*`), each call's exception
//! cleared and taken as a failure.

use android_activity::AndroidApp;
use jni_sys::{jboolean, jclass, jint, jmethodID, jobject, jvalue, JNIEnv, JavaVM, JNI_OK};
use std::ffi::CString;

/// The JNI environment of the calling thread (attached when it is not yet).
struct Env(*mut JNIEnv);

impl Env {
    fn get(app: &AndroidApp) -> Option<Env> {
        let vm = app.vm_as_ptr() as *mut JavaVM;
        if vm.is_null() {
            return None;
        }
        let mut env: *mut std::ffi::c_void = std::ptr::null_mut();
        // SAFETY: `vm` is the process's JavaVM (android-activity's); GetEnv and
        // AttachCurrentThread are its invocation interface.
        unsafe {
            let get = (**vm).GetEnv?;
            if get(vm, &mut env, jni_sys::JNI_VERSION_1_6) != JNI_OK {
                let attach = (**vm).AttachCurrentThread?;
                if attach(vm, &mut env, std::ptr::null_mut()) != JNI_OK {
                    return None;
                }
            }
        }
        (!env.is_null()).then_some(Env(env as *mut JNIEnv))
    }

    /// A pending exception cleared: whether there was one.
    fn failed(&self) -> bool {
        // SAFETY: a valid JNIEnv of this thread.
        unsafe {
            let env = self.0;
            let check = (**env).ExceptionCheck.unwrap();
            if check(env) != 0 {
                (**env).ExceptionClear.unwrap()(env);
                return true;
            }
        }
        false
    }

    fn class(&self, name: &str) -> Option<jclass> {
        let c = CString::new(name).ok()?;
        // SAFETY: as above.
        let class = unsafe { (**self.0).FindClass.unwrap()(self.0, c.as_ptr()) };
        (!self.failed() && !class.is_null()).then_some(class)
    }

    fn method(&self, class: jclass, name: &str, sig: &str, stat: bool) -> Option<jmethodID> {
        let (n, s) = (CString::new(name).ok()?, CString::new(sig).ok()?);
        // SAFETY: as above.
        let m = unsafe {
            if stat {
                (**self.0).GetStaticMethodID.unwrap()(self.0, class, n.as_ptr(), s.as_ptr())
            } else {
                (**self.0).GetMethodID.unwrap()(self.0, class, n.as_ptr(), s.as_ptr())
            }
        };
        (!self.failed() && !m.is_null()).then_some(m)
    }

    fn string(&self, s: &str) -> Option<jobject> {
        let c = CString::new(s.replace('\0', " ")).ok()?;
        // SAFETY: as above.
        let j = unsafe { (**self.0).NewStringUTF.unwrap()(self.0, c.as_ptr()) };
        (!self.failed() && !j.is_null()).then_some(j)
    }

    fn new_object(&self, class: jclass, ctor: jmethodID, args: &[jvalue]) -> Option<jobject> {
        // SAFETY: as above; `args` match the constructor's signature (the callers').
        let o = unsafe { (**self.0).NewObjectA.unwrap()(self.0, class, ctor, args.as_ptr()) };
        (!self.failed() && !o.is_null()).then_some(o)
    }

    fn call_object(&self, o: jobject, m: jmethodID, args: &[jvalue]) -> Option<jobject> {
        // SAFETY: as above.
        let r = unsafe { (**self.0).CallObjectMethodA.unwrap()(self.0, o, m, args.as_ptr()) };
        (!self.failed()).then_some(r)
    }

    fn call_void(&self, o: jobject, m: jmethodID, args: &[jvalue]) -> bool {
        // SAFETY: as above.
        unsafe { (**self.0).CallVoidMethodA.unwrap()(self.0, o, m, args.as_ptr()) };
        !self.failed()
    }

    fn call_float(&self, o: jobject, m: jmethodID, args: &[jvalue]) -> Option<f32> {
        // SAFETY: as above.
        let r = unsafe { (**self.0).CallFloatMethodA.unwrap()(self.0, o, m, args.as_ptr()) };
        (!self.failed()).then_some(r)
    }

    fn static_object(&self, c: jclass, m: jmethodID, args: &[jvalue]) -> Option<jobject> {
        // SAFETY: as above.
        let r = unsafe { (**self.0).CallStaticObjectMethodA.unwrap()(self.0, c, m, args.as_ptr()) };
        (!self.failed() && !r.is_null()).then_some(r)
    }

    fn static_bool(&self, c: jclass, m: jmethodID) -> Option<bool> {
        // SAFETY: as above.
        let r: jboolean =
            unsafe { (**self.0).CallStaticBooleanMethodA.unwrap()(self.0, c, m, std::ptr::null()) };
        (!self.failed()).then_some(r != 0)
    }

    /// Local references dropped in a frame of their own around `f`.
    fn frame<R>(&self, f: impl FnOnce(&Env) -> R) -> R {
        // SAFETY: as above.
        unsafe { (**self.0).PushLocalFrame.unwrap()(self.0, 64) };
        let r = f(self);
        unsafe { (**self.0).PopLocalFrame.unwrap()(self.0, std::ptr::null_mut()) };
        r
    }
}

fn obj(l: jobject) -> jvalue {
    jvalue { l }
}
fn int(i: jint) -> jvalue {
    jvalue { i }
}
fn float(f: f32) -> jvalue {
    jvalue { f }
}

/// Whether the app has "All files access" (`Environment.isExternalStorageManager`, Android 11
/// and later); `None` when it could not be asked.
pub fn all_files_access(app: &AndroidApp) -> Option<bool> {
    let env = Env::get(app)?;
    env.frame(|env| {
        let c = env.class("android/os/Environment")?;
        let m = env.method(c, "isExternalStorageManager", "()Z", true)?;
        env.static_bool(c, m)
    })
}

/// Opens the system's "All files access" page for this app
/// (`Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION` with `package:<name>`), or the list of
/// every app's access where that page is missing. Whether a page was opened.
pub fn open_all_files_access(app: &AndroidApp, package: &str) -> bool {
    let Some(env) = Env::get(app) else {
        return false;
    };
    let activity = app.activity_as_ptr() as jobject;
    if activity.is_null() {
        return false;
    }
    env.frame(|env| {
        let open = |action: &str, uri: Option<&str>| -> Option<()> {
            let intent_class = env.class("android/content/Intent")?;
            let action = env.string(action)?;
            let intent = match uri {
                Some(u) => {
                    let uri_class = env.class("android/net/Uri")?;
                    let parse = env.method(
                        uri_class,
                        "parse",
                        "(Ljava/lang/String;)Landroid/net/Uri;",
                        true,
                    )?;
                    let uri = env.static_object(uri_class, parse, &[obj(env.string(u)?)])?;
                    let ctor = env.method(
                        intent_class,
                        "<init>",
                        "(Ljava/lang/String;Landroid/net/Uri;)V",
                        false,
                    )?;
                    env.new_object(intent_class, ctor, &[obj(action), obj(uri)])?
                }
                None => {
                    let ctor =
                        env.method(intent_class, "<init>", "(Ljava/lang/String;)V", false)?;
                    env.new_object(intent_class, ctor, &[obj(action)])?
                }
            };
            let flags = env.method(
                intent_class,
                "addFlags",
                "(I)Landroid/content/Intent;",
                false,
            )?;
            // FLAG_ACTIVITY_NEW_TASK.
            env.call_object(intent, flags, &[int(0x1000_0000)])?;
            let activity_class = env.class("android/app/Activity")?;
            let start = env.method(
                activity_class,
                "startActivity",
                "(Landroid/content/Intent;)V",
                false,
            )?;
            env.call_void(activity, start, &[obj(intent)]).then_some(())
        };
        open(
            "android.settings.MANAGE_APP_ALL_FILES_ACCESS_PERMISSION",
            Some(&format!("package:{package}")),
        )
        .or_else(|| open("android.settings.MANAGE_ALL_FILES_ACCESS_PERMISSION", None))
        .is_some()
    })
}

/// The message `lines` drawn in the activity's window: white text (Android's default font) on a
/// dark background, long lines wrapped. Whether it was drawn.
///
/// The window is drawn by the CPU here, so a game started later in the same window could not make
/// its Vulkan surface there: the app only draws when it will not start the game in this window
/// (it waits for the next one, which Android makes after the settings page).
pub fn draw_message(app: &AndroidApp, lines: &[String]) -> bool {
    let Some(window) = app.native_window() else {
        return false;
    };
    let (w, h) = (window.width().max(1), window.height().max(1));
    let Some(pixels) = render(app, w, h, lines) else {
        return false;
    };
    if window
        .set_buffers_geometry(
            w,
            h,
            Some(ndk::hardware_buffer_format::HardwareBufferFormat::R8G8B8A8_UNORM),
        )
        .is_err()
    {
        return false;
    }
    let Ok(mut buffer) = window.lock(None) else {
        return false;
    };
    let (bw, bh, stride) = (buffer.width(), buffer.height(), buffer.stride());
    let bits = buffer.bits() as *mut u8;
    for y in 0..bh.min(h as usize) {
        for x in 0..bw.min(w as usize) {
            let argb = pixels[y * w as usize + x] as u32;
            let at = (y * stride + x) * 4;
            // SAFETY: inside the locked buffer (stride * height pixels of 4 bytes).
            unsafe {
                *bits.add(at) = (argb >> 16) as u8;
                *bits.add(at + 1) = (argb >> 8) as u8;
                *bits.add(at + 2) = argb as u8;
                *bits.add(at + 3) = 0xff;
            }
        }
    }
    drop(buffer);
    true
}

/// The message in a `w` by `h` bitmap (`android.graphics`), its pixels (ARGB).
fn render(app: &AndroidApp, w: i32, h: i32, lines: &[String]) -> Option<Vec<i32>> {
    let env = Env::get(app)?;
    env.frame(|env| {
        let config_class = env.class("android/graphics/Bitmap$Config")?;
        let name = CString::new("ARGB_8888").ok()?;
        let sig = CString::new("Landroid/graphics/Bitmap$Config;").ok()?;
        // SAFETY: a valid JNIEnv of this thread; the field is the enum's constant.
        let config = unsafe {
            let id = (**env.0).GetStaticFieldID.unwrap()(
                env.0,
                config_class,
                name.as_ptr(),
                sig.as_ptr(),
            );
            if env.failed() || id.is_null() {
                return None;
            }
            (**env.0).GetStaticObjectField.unwrap()(env.0, config_class, id)
        };
        if env.failed() || config.is_null() {
            return None;
        }
        let bitmap_class = env.class("android/graphics/Bitmap")?;
        let create = env.method(
            bitmap_class,
            "createBitmap",
            "(IILandroid/graphics/Bitmap$Config;)Landroid/graphics/Bitmap;",
            true,
        )?;
        let bitmap = env.static_object(bitmap_class, create, &[int(w), int(h), obj(config)])?;
        let canvas_class = env.class("android/graphics/Canvas")?;
        let canvas_new = env.method(
            canvas_class,
            "<init>",
            "(Landroid/graphics/Bitmap;)V",
            false,
        )?;
        let canvas = env.new_object(canvas_class, canvas_new, &[obj(bitmap)])?;
        let draw_color = env.method(canvas_class, "drawColor", "(I)V", false)?;
        env.call_void(canvas, draw_color, &[int(0xff10_1418u32 as i32)]);
        let paint_class = env.class("android/graphics/Paint")?;
        let paint_new = env.method(paint_class, "<init>", "(I)V", false)?;
        // ANTI_ALIAS_FLAG.
        let paint = env.new_object(paint_class, paint_new, &[int(1)])?;
        let set_color = env.method(paint_class, "setColor", "(I)V", false)?;
        let set_size = env.method(paint_class, "setTextSize", "(F)V", false)?;
        let measure = env.method(paint_class, "measureText", "(Ljava/lang/String;)F", false)?;
        let draw_text = env.method(
            canvas_class,
            "drawText",
            "(Ljava/lang/String;FFLandroid/graphics/Paint;)V",
            false,
        )?;
        let size = (h.min(w) as f32 / 26.0).clamp(18.0, 64.0);
        env.call_void(paint, set_size, &[float(size)]);
        let margin = size * 1.2;
        let room = w as f32 - 2.0 * margin;
        let mut y = margin + size;
        for (i, line) in lines.iter().enumerate() {
            // The first line a heading, in a warmer colour.
            let colour = if i == 0 { 0xffff_c860u32 } else { 0xffe8_e8e8 };
            env.call_void(paint, set_color, &[int(colour as i32)]);
            for piece in wrap(line, room, |s| {
                env.frame(|env| {
                    env.string(s)
                        .and_then(|j| env.call_float(paint, measure, &[obj(j)]))
                        .unwrap_or(s.len() as f32 * size * 0.55)
                })
            }) {
                env.frame(|env| {
                    if let Some(j) = env.string(&piece) {
                        env.call_void(
                            canvas,
                            draw_text,
                            &[obj(j), float(margin), float(y), obj(paint)],
                        );
                    }
                });
                y += size * 1.35;
            }
            if line.is_empty() {
                y += size * 0.2;
            }
        }
        // The pixels (ARGB ints).
        let get_pixels = env.method(bitmap_class, "getPixels", "([IIIIIII)V", false)?;
        let n = (w * h) as usize;
        // SAFETY: a valid JNIEnv; the array is the bitmap's size.
        let array = unsafe { (**env.0).NewIntArray.unwrap()(env.0, n as jint) };
        if env.failed() || array.is_null() {
            return None;
        }
        if !env.call_void(
            bitmap,
            get_pixels,
            &[obj(array), int(0), int(w), int(0), int(0), int(w), int(h)],
        ) {
            return None;
        }
        let mut out = vec![0i32; n];
        // SAFETY: `out` holds n ints, the array's length.
        unsafe {
            (**env.0).GetIntArrayRegion.unwrap()(env.0, array, 0, n as jint, out.as_mut_ptr())
        };
        (!env.failed()).then_some(out)
    })
}

/// `line` cut at spaces (or anywhere, for a word longer than the room) into pieces that `width`
/// measures within `room`.
pub fn wrap(line: &str, room: f32, mut width: impl FnMut(&str) -> f32) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for word in line.split(' ') {
        let tried = if current.is_empty() {
            word.to_string()
        } else {
            format!("{current} {word}")
        };
        if width(&tried) <= room || current.is_empty() {
            current = tried;
            // A word too long alone: cut by characters.
            while width(&current) > room && current.chars().count() > 1 {
                let mut cut = current.chars().count() - 1;
                while cut > 1 && width(&current.chars().take(cut).collect::<String>()) > room {
                    cut -= 1;
                }
                let head: String = current.chars().take(cut).collect();
                let tail: String = current.chars().skip(cut).collect();
                out.push(head);
                current = tail;
            }
        } else {
            out.push(std::mem::take(&mut current));
            current = word.to_string();
        }
    }
    out.push(current);
    out
}
