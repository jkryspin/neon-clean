Add-Type @'
using System;using System.Runtime.InteropServices;using System.Text;
public class MS {
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out R r);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint dx,uint dy,uint d,UIntPtr e);
  [DllImport("user32.dll")] public static extern void keybd_event(byte v,byte s,uint f,UIntPtr e);
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h,int c);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h,out R r);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h,out uint pid);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint a,uint b,bool f);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();
  [DllImport("user32.dll")] public static extern int GetWindowText(IntPtr h,StringBuilder s,int n);
  public struct R{public int L,T,Rt,B;}
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h,IntPtr after,int x,int y,int cx,int cy,uint flags);
  public delegate bool EnumProc(IntPtr h,IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb,IntPtr l);
}
'@

function Find-Window {
  param([int]$target,[switch]$All)
  $global:found=[IntPtr]::Zero
  $cb=[MS+EnumProc]{ param($h,$l) $pp=0; [void][MS]::GetWindowThreadProcessId($h,[ref]$pp); if($pp -eq $target -and ($All -or [MS]::IsWindowVisible($h))){ $global:found=[IntPtr]$h; return $false }; return $true }
  [void][MS]::EnumWindows($cb,[IntPtr]::Zero)
  $global:found
}

function Get-Game { Get-Process MapleStory -ErrorAction SilentlyContinue | Where-Object { $_.MainWindowHandle -ne 0 } | Select-Object -First 1 }

function Get-Rect {
  param($h)
  if(-not $h){ $p=Get-Game; if($p){$h=[IntPtr]$p.MainWindowHandle} }
  if(-not $h){ $p=Get-Process MapleStory -ErrorAction SilentlyContinue; if($p){ $h=Find-Window $p.Id } }
  if(-not $h){ return $null }
  $r=New-Object MS+R
  [void][MS]::GetWindowRect($h,[ref]$r)
  [pscustomobject]@{ L=$r.L; T=$r.T; W=$r.Rt-$r.L; H=$r.B-$r.T }
}

function Focus-Game {
  param($h)
  if(-not $h){ $p=Get-Game; if($p){$h=[IntPtr]$p.MainWindowHandle} }
  if(-not $h){ $p=Get-Process MapleStory -ErrorAction SilentlyContinue; if($p){ $h=Find-Window $p.Id } }
  if(-not $h){ throw "no game window" }
  $fg=[MS]::GetForegroundWindow()
  $tid1=[MS]::GetWindowThreadProcessId($fg,[ref]$null)
  $tid2=[MS]::GetWindowThreadProcessId($h,[ref]$null)
  [void][MS]::AttachThreadInput([MS]::GetCurrentThreadId(),$tid1,$true)
  [void][MS]::AttachThreadInput([MS]::GetCurrentThreadId(),$tid2,$true)
  [void][MS]::SetForegroundWindow($h)
  [void][MS]::AttachThreadInput([MS]::GetCurrentThreadId(),$tid1,$false)
  [void][MS]::AttachThreadInput([MS]::GetCurrentThreadId(),$tid2,$false)
  Start-Sleep -Milliseconds 150
}

function Bring-Top {
  param($h)
  if(-not $h){ $p=Get-Process MapleStory -ErrorAction SilentlyContinue; if($p){ $h=Find-Window $p.Id -All } }
  if(-not $h){ throw "no game window" }
  [void][MS]::ShowWindow($h,5)
  [void][MS]::SetWindowPos($h,[IntPtr](-1),0,0,0,0,0x0001 -bor 0x0002 -bor 0x0040)
  Focus-Game $h
}

function Click-Rel {
  param($h,[double]$rx,[double]$ry)
  if(-not $h){ throw "no hwnd" }
  $r=Get-Rect $h
  $x=[int]($r.L+$rx*$r.W); $y=[int]($r.T+$ry*$r.H)
  [void][MS]::SetCursorPos($x,$y); Start-Sleep -Milliseconds 100
  [MS]::mouse_event(0x02,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 70
  [MS]::mouse_event(0x04,0,0,0,[UIntPtr]::Zero)
  "click $x,$y"
}

function Double-Click-Rel {
  param($h,[double]$rx,[double]$ry)
  $r=Get-Rect $h
  $x=[int]($r.L+$rx*$r.W); $y=[int]($r.T+$ry*$r.H)
  [void][MS]::SetCursorPos($x,$y); Start-Sleep -Milliseconds 100
  for($i=0;$i -lt 2;$i++){
    [MS]::mouse_event(0x02,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 60
    [MS]::mouse_event(0x04,0,0,0,[UIntPtr]::Zero); Start-Sleep -Milliseconds 60
  }
  "dblclick $x,$y"
}

function Hold-Key {
  param([byte]$vk,[int]$ms=400)
  [MS]::keybd_event($vk,0,0,[UIntPtr]::Zero)
  Start-Sleep -Milliseconds $ms
  [MS]::keybd_event($vk,0,2,[UIntPtr]::Zero)
  "held vk=$vk ${ms}ms"
}

function Shot {
  param([string]$name="shot")
  $path="C:\Users\John\AppData\Local\Temp\opencode\$name.jpg"
  Add-Type -AssemblyName System.Drawing,System.Windows.Forms
  $r=[System.Windows.Forms.Screen]::PrimaryScreen.Bounds
  $bmp=New-Object System.Drawing.Bitmap($r.Width,$r.Height)
  $g=[System.Drawing.Graphics]::FromImage($bmp)
  $g.CopyFromScreen(0,0,0,0,$bmp.Size)
  $w=960; $h=[int]($r.Height*$w/$r.Width)
  $s=New-Object System.Drawing.Bitmap($w,$h)
  $g2=[System.Drawing.Graphics]::FromImage($s)
  $g2.InterpolationMode='HighQualityBicubic'
  $g2.DrawImage($bmp,0,0,$w,$h)
  $enc=[System.Drawing.Imaging.ImageCodecInfo]::GetImageEncoders()|Where-Object{$_.MimeType -eq 'image/jpeg'}
  $p=New-Object System.Drawing.Imaging.EncoderParameters(1)
  $p.Param[0]=New-Object System.Drawing.Imaging.EncoderParameter([System.Drawing.Imaging.Encoder]::Quality,78)
  $s.Save($path,$enc,$p)
  $g.Dispose();$g2.Dispose();$bmp.Dispose();$s.Dispose()
  $path
}
