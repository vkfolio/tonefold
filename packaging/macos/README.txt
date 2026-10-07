Tonefold for macOS

1. Drag Tonefold into Applications.

2. Open it. This build is not notarized by Apple yet, so the first time macOS
   says it can't check the app:
     - macOS 15 and later: open System Settings > Privacy & Security, scroll down
       and click "Open Anyway" next to Tonefold.
     - macOS 14 and earlier: right-click Tonefold in Applications, choose Open,
       then Open again.
   Or, in Terminal:  xattr -dr com.apple.quarantine /Applications/Tonefold.app

3. The chat composer needs Node.js 20 or newer (https://nodejs.org) and either
   Claude Code signed in on this Mac or an Ollama server. The first chat message
   installs the composer's dependencies, which takes about a minute. Generating,
   playing, editing and exporting work without any of this.

Optional: use Tonefold inside a DAW
   Copy the files in "DAW plugins" to:
     Tonefold.clap  ->  ~/Library/Audio/Plug-Ins/CLAP
     Tonefold.vst3  ->  ~/Library/Audio/Plug-Ins/VST3
   (In Finder, press Shift-Command-G and paste the folder path.)
   Then rescan plugins in your DAW.

Your songs, exports and logs live in ~/Library/Application Support/Tonefold.

Docs and source: https://github.com/vkfolio/tonefold
