# Tiny UI automation helper for the standalone window: move, click, type, key, shot.
param([string[]]$Steps)
$env:TMP = Join-Path (Split-Path $PSScriptRoot -Parent) "target"; $env:TEMP = $env:TMP
Add-Type -AssemblyName System.Windows.Forms
Add-Type -AssemblyName System.Drawing
Add-Type @"
using System; using System.Runtime.InteropServices;
public class W {
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint f);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint x, uint y, uint d, UIntPtr e);
}
"@
[W]::SetProcessDPIAware() | Out-Null
$p = Get-Process tonefold-standalone -ErrorAction Stop | Select-Object -First 1
$h = $p.MainWindowHandle
[W]::SetForegroundWindow($h) | Out-Null
$i = 0
while ($i -lt $Steps.Length) {
  $s = $Steps[$i]; $i++
  switch ($s) {
    "move"  { [W]::SetWindowPos($h, [IntPtr](-1), 0, 0, 0, 0, 0x0001 -bor 0x0040) | Out-Null; Start-Sleep -Milliseconds 300 }
    "click" { $x=[int]$Steps[$i]; $y=[int]$Steps[$i+1]; $i+=2
              [W]::SetCursorPos($x,$y) | Out-Null; Start-Sleep -Milliseconds 80
              [W]::mouse_event(2,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 60; [W]::mouse_event(4,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 250 }
    "dclick"{ $x=[int]$Steps[$i]; $y=[int]$Steps[$i+1]; $i+=2
              [W]::SetCursorPos($x,$y) | Out-Null; Start-Sleep -Milliseconds 80
              [W]::mouse_event(2,0,0,0,[UIntPtr]::Zero); [W]::mouse_event(4,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 90
              [W]::mouse_event(2,0,0,0,[UIntPtr]::Zero); [W]::mouse_event(4,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 250 }
    "rclick"{ $x=[int]$Steps[$i]; $y=[int]$Steps[$i+1]; $i+=2
              [W]::SetCursorPos($x,$y) | Out-Null; Start-Sleep -Milliseconds 80
              [W]::mouse_event(8,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 60; [W]::mouse_event(16,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 250 }
    "drag"  { $x=[int]$Steps[$i]; $y=[int]$Steps[$i+1]; $x2=[int]$Steps[$i+2]; $y2=[int]$Steps[$i+3]; $i+=4
              [W]::SetCursorPos($x,$y) | Out-Null; Start-Sleep -Milliseconds 80
              [W]::mouse_event(2,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 100
              $nsteps=12; for ($k=1; $k -le $nsteps; $k++) { $cx=$x+($x2-$x)*$k/$nsteps; $cy=$y+($y2-$y)*$k/$nsteps; [W]::SetCursorPos([int]$cx,[int]$cy) | Out-Null; Start-Sleep -Milliseconds 25 }
              Start-Sleep -Milliseconds 100; [W]::mouse_event(4,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 250 }
    "type"  { $t=$Steps[$i]; $i++; [System.Windows.Forms.SendKeys]::SendWait($t); Start-Sleep -Milliseconds 200 }
    "key"   { $k=$Steps[$i]; $i++; [System.Windows.Forms.SendKeys]::SendWait("{$k}"); Start-Sleep -Milliseconds 200 }
    "sleep" { $ms=[int]$Steps[$i]; $i++; Start-Sleep -Milliseconds $ms }
    "shot"  { $f=$Steps[$i]; $i++
              $b = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds
              $bmp = New-Object System.Drawing.Bitmap $b.Width, $b.Height
              $g = [System.Drawing.Graphics]::FromImage($bmp); $g.CopyFromScreen($b.Location, [System.Drawing.Point]::Empty, $b.Size)
              $bmp.Save($f, [System.Drawing.Imaging.ImageFormat]::Png); $g.Dispose()
              # Also a 1600px-wide copy (viewer limit is 2000px).
              $sw = 1600; $sh = [int]($b.Height * 1600 / $b.Width)
              $small = New-Object System.Drawing.Bitmap $sw, $sh; $g2 = [System.Drawing.Graphics]::FromImage($small); $g2.InterpolationMode = 'HighQualityBicubic'
              $g2.DrawImage($bmp, 0, 0, $sw, $sh); $small.Save(($f -replace '\.png$', 's.png'), [System.Drawing.Imaging.ImageFormat]::Png); $g2.Dispose(); $small.Dispose(); $bmp.Dispose()
              Write-Host "shot $f $($b.Width)x$($b.Height)" }
  }
}

