# Runtime-only balance checks

ZZZManager will run scheduled balance checks only while the Tauri desktop app is running in the first release. It will not install a background service, register operating system scheduled tasks, or guarantee checks after the app is closed, because cross-platform background execution would add significant complexity before the core account, threshold, history, and webhook workflow is proven.
