# JNI locates this callback by its fully qualified Java name.
-keep class com.jinziyou.ideall.IdeallNativeActivity {
    native <methods>;
    public void showIdeallTextInput(...);
    public void updateIdeallTextSelection(...);
    public void hideIdeallTextInput();
}
