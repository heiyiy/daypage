param([int]$AppPid,[ValidateSet('status','desktop','journal','widget','restore','winkeyd')][string]$Action='status')
$ErrorActionPreference='Stop'
Add-Type @'
using System;using System.Text;using System.Runtime.InteropServices;
public static class DesktopProbe {
 delegate bool EnumProc(IntPtr h,IntPtr p);
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p,IntPtr l);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll")] static extern bool IsWindowVisible(IntPtr h);
 [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr h,int n);
 [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] static extern int GetWindowLong(IntPtr h,int n);
 public static IntPtr Find(int pid,bool journal){IntPtr result=IntPtr.Zero;EnumWindows((h,p)=>{uint owner;GetWindowThreadProcessId(h,out owner);var text=new StringBuilder(256);GetWindowText(h,text,256);if(owner==pid&&text.ToString().Contains(journal?"日记与日历":"待办")){result=h;return false;}return true;},IntPtr.Zero);return result;}
 [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
 [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a,uint b,bool attach);
 [DllImport("user32.dll")] static extern IntPtr GetShellWindow();
 [DllImport("user32.dll")] static extern int GetSystemMetrics(int n);
 [DllImport("user32.dll")] static extern void keybd_event(byte key,byte scan,uint flags,UIntPtr extra);
 public static void ShowDesktopKey(){keybd_event(0x5B,0,0,UIntPtr.Zero);keybd_event(0x44,0,0,UIntPtr.Zero);keybd_event(0x44,0,2,UIntPtr.Zero);keybd_event(0x5B,0,2,UIntPtr.Zero);}
 public static void FocusDesktop(){SetCursorPos(GetSystemMetrics(0)-80,GetSystemMetrics(1)-100);mouse_event(2,0,0,0,UIntPtr.Zero);mouse_event(4,0,0,0,UIntPtr.Zero);}
 [StructLayout(LayoutKind.Sequential)] struct Rect { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
 public static void Focus(int pid,bool journal){var h=Find(pid,journal);FocusHandle(h);Rect r;GetWindowRect(h,out r);SetCursorPos(r.Left+25,r.Top+70);mouse_event(2,0,0,0,UIntPtr.Zero);mouse_event(4,0,0,0,UIntPtr.Zero);}
 static void FocusHandle(IntPtr h){if(h==IntPtr.Zero)throw new Exception("Test window missing");uint owner;uint foreground=GetWindowThreadProcessId(GetForegroundWindow(),out owner),current=GetCurrentThreadId();bool attached=foreground!=current&&AttachThreadInput(current,foreground,true);try{ShowWindow(h,9);SetForegroundWindow(h);}finally{if(attached)AttachThreadInput(current,foreground,false);}}
 public static string Status(int pid){var h=Find(pid,false);var f=GetForegroundWindow();var name=new StringBuilder(256);GetClassName(f,name,256);return "{\"visible\":"+(IsWindowVisible(h)?"true":"false")+",\"topmost\":"+((GetWindowLong(h,-20)&8)!=0?"true":"false")+",\"widgetFocused\":"+(h==f?"true":"false")+",\"foregroundClass\":\""+name+"\"}";}
}
'@
switch($Action){
 'desktop' {(New-Object -ComObject Shell.Application).MinimizeAll(); Start-Sleep -Milliseconds 500; [DesktopProbe]::FocusDesktop(); Start-Sleep -Milliseconds 300}
 'winkeyd' {[DesktopProbe]::ShowDesktopKey(); Start-Sleep -Milliseconds 700}
 'restore' {(New-Object -ComObject Shell.Application).UndoMinimizeAll(); Start-Sleep -Milliseconds 200}
 'journal' {[DesktopProbe]::Focus($AppPid,$true); Start-Sleep -Milliseconds 300}
 'widget' {[DesktopProbe]::Focus($AppPid,$false); Start-Sleep -Milliseconds 300}
}
[DesktopProbe]::Status($AppPid)
