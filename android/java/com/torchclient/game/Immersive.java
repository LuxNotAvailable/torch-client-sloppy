package com.torchclient.game;

import android.app.Activity;
import android.os.Build;
import android.view.View;
import android.view.Window;
import android.view.WindowInsets;
import android.view.WindowInsetsController;

public final class Immersive {
    private static boolean listening;

    public static void install(final Activity activity) {
        activity.runOnUiThread(() -> {
            final Window window = activity.getWindow();
            hide(window);
            if (!listening) {
                listening = true;
                window.getDecorView().getViewTreeObserver().addOnWindowFocusChangeListener(focused -> {
                    if (focused) {
                        hide(window);
                    }
                });
            }
        });
    }

    private static void hide(Window window) {
        if (Build.VERSION.SDK_INT >= 30) {
            WindowInsetsController controller = window.getInsetsController();
            if (controller == null) {
                return;
            }
            controller.setSystemBarsBehavior(WindowInsetsController.BEHAVIOR_SHOW_TRANSIENT_BARS_BY_SWIPE);
            controller.hide(WindowInsets.Type.navigationBars() | WindowInsets.Type.statusBars());
        } else {
            window.getDecorView().setSystemUiVisibility(
                    View.SYSTEM_UI_FLAG_IMMERSIVE_STICKY
                    | View.SYSTEM_UI_FLAG_HIDE_NAVIGATION
                    | View.SYSTEM_UI_FLAG_FULLSCREEN
                    | View.SYSTEM_UI_FLAG_LAYOUT_STABLE
                    | View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION
                    | View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN);
        }
    }
}
