use jni::{
    Env, JValue, JavaVM,
    errors::Result,
    jni_sig, jni_str,
    objects::{JClass, JObject},
};

pub(crate) fn with_activity<T>(
    f: impl FnOnce(&mut Env<'_>, &JObject<'_>) -> Result<T>,
) -> Option<Result<T>> {
    let app = bevy::android::ANDROID_APP.get()?;
    let vm = unsafe { JavaVM::from_raw(app.vm_as_ptr().cast()) };
    Some(vm.attach_current_thread(|env| {
        let activity = unsafe { JObject::from_raw(env, app.activity_as_ptr().cast()) };
        let result = f(env, &activity);
        if result.is_err() {
            env.exception_clear();
        }
        result
    }))
}

pub(crate) fn app_class<'local>(
    env: &mut Env<'local>,
    activity: &JObject<'_>,
    name: &str,
) -> Result<JClass<'local>> {
    let loader = env
        .call_method(
            activity,
            jni_str!("getClassLoader"),
            jni_sig!("()Ljava/lang/ClassLoader;"),
            &[],
        )?
        .l()?;
    let name: JObject = env.new_string(name)?.into();
    let class = env
        .call_method(
            &loader,
            jni_str!("loadClass"),
            jni_sig!("(Ljava/lang/String;)Ljava/lang/Class;"),
            &[JValue::Object(&name)],
        )?
        .l()?;
    env.cast_local::<JClass>(class)
}
