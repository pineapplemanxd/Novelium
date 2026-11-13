#import <UIKit/UIKit.h>
#import <WebKit/WebKit.h>
#include "bindings/bindings.h"

static UIColor *NoveliumBackgroundColor(void) {
	return [UIColor colorWithRed:11.0 / 255.0 green:13.0 / 255.0 blue:17.0 / 255.0 alpha:1.0];
}

static WKWebView *FindNoveliumWebView(UIView *view) {
	if (!view) {
		return nil;
	}
	if ([view isKindOfClass:[WKWebView class]]) {
		return (WKWebView *)view;
	}
	for (UIView *subview in view.subviews) {
		WKWebView *found = FindNoveliumWebView(subview);
		if (found) {
			return found;
		}
	}
	return nil;
}

static void ConfigureNoveliumWindows(void) {
	UIColor *bgColor = NoveliumBackgroundColor();
	NSMutableSet<UIWindow *> *windows = [NSMutableSet set];

	for (UIScene *scene in UIApplication.sharedApplication.connectedScenes) {
		if ([scene isKindOfClass:[UIWindowScene class]]) {
			UIWindowScene *windowScene = (UIWindowScene *)scene;
			for (UIWindow *window in windowScene.windows) {
				NSString *cls = NSStringFromClass([window class]);
				if ([cls containsString:@"TextEffects"] || [cls containsString:@"Keyboard"] || !window.rootViewController) {
					continue;
				}
				[windows addObject:window];
			}
		}
	}

	for (UIWindow *window in windows) {
		CGRect screenBounds = window.screen ? window.screen.bounds : UIScreen.mainScreen.bounds;
		if (!CGRectEqualToRect(window.frame, screenBounds)) {
			window.frame = screenBounds;
		}
		window.backgroundColor = bgColor;
		window.overrideUserInterfaceStyle = UIUserInterfaceStyleDark;

		UIEdgeInsets hardwareSafe = window.safeAreaInsets;
		UIViewController *rootVC = window.rootViewController;
		if (rootVC) {
			UIEdgeInsets negSafe = UIEdgeInsetsMake(-hardwareSafe.top, -hardwareSafe.left, -hardwareSafe.bottom, -hardwareSafe.right);
			if (!UIEdgeInsetsEqualToEdgeInsets(rootVC.additionalSafeAreaInsets, negSafe)) {
				rootVC.additionalSafeAreaInsets = negSafe;
			}
			if (rootVC.view) {
				UIView *rootView = rootVC.view;
				if (!CGRectEqualToRect(rootView.frame, screenBounds)) {
					rootView.frame = screenBounds;
				}
				rootView.autoresizingMask = UIViewAutoresizingFlexibleWidth | UIViewAutoresizingFlexibleHeight;
				rootView.backgroundColor = bgColor;
			}
		}

		WKWebView *webView = FindNoveliumWebView(window);
		if (webView) {
			BOOL frameChanged = !CGRectEqualToRect(webView.frame, screenBounds);
			if (frameChanged) {
				webView.frame = screenBounds;
			}
			webView.autoresizingMask = UIViewAutoresizingFlexibleWidth | UIViewAutoresizingFlexibleHeight;
			webView.opaque = NO;
			webView.backgroundColor = bgColor;
			webView.scrollView.backgroundColor = bgColor;
			webView.scrollView.contentInsetAdjustmentBehavior = UIScrollViewContentInsetAdjustmentNever;
			webView.scrollView.contentInset = UIEdgeInsetsZero;
			webView.scrollView.scrollIndicatorInsets = UIEdgeInsetsZero;
			webView.scrollView.bounces = NO;
			webView.scrollView.alwaysBounceVertical = NO;
			webView.scrollView.alwaysBounceHorizontal = NO;

			CGFloat topInset = hardwareSafe.top > 0 ? hardwareSafe.top : 59.0;
			CGFloat bottomInset = hardwareSafe.bottom > 0 ? hardwareSafe.bottom : 34.0;
			CGFloat fullHeight = screenBounds.size.height;

			NSString *js = [NSString stringWithFormat:
				@"(function() {"
				 "  if (!document || !document.documentElement) return;"
				 "  document.documentElement.style.setProperty('--app-height', '%.0fpx');"
				 "  document.documentElement.style.setProperty('--native-safe-top', '%.0fpx');"
				 "  document.documentElement.style.setProperty('--native-safe-bottom', '%.0fpx');"
				 "  if (%@) { window.dispatchEvent(new Event('resize')); }"
				 "})()",
				fullHeight,
				topInset,
				bottomInset,
				frameChanged ? @"true" : @"false"
			];
			[webView evaluateJavaScript:js completionHandler:nil];
		}
	}
}

static void ScheduleNoveliumWindowPass(void) {
	ConfigureNoveliumWindows();
	NSArray<NSNumber *> *delaysMs = @[@50, @150, @350, @750, @1500, @3000];
	for (NSNumber *delay in delaysMs) {
		dispatch_after(dispatch_time(DISPATCH_TIME_NOW, (int64_t)([delay integerValue] * NSEC_PER_MSEC)), dispatch_get_main_queue(), ^{
			ConfigureNoveliumWindows();
		});
	}
}

int main(int argc, char * argv[]) {
	@autoreleasepool {
		[UIScrollView appearance].contentInsetAdjustmentBehavior = UIScrollViewContentInsetAdjustmentNever;

		NSNotificationCenter *center = [NSNotificationCenter defaultCenter];
		NSArray<NSNotificationName> *notifications = @[
			UIWindowDidBecomeKeyNotification,
			UIWindowDidBecomeVisibleNotification,
			UIApplicationDidBecomeActiveNotification,
			UIDeviceOrientationDidChangeNotification
		];
		for (NSNotificationName name in notifications) {
			[center addObserverForName:name
								object:nil
								 queue:[NSOperationQueue mainQueue]
							usingBlock:^(__unused NSNotification *note) {
				ScheduleNoveliumWindowPass();
			}];
		}
	}
	ffi::start_app();
	return 0;
}
