param([int]$AppPid,[ValidateSet('status','desktop','journal','widget','restore','winkeyd','minimize-journal','hide-widget','minimize-widget','taskbar','inventory')][string]$Action='status')
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
 [DllImport("user32.dll")] static extern bool IsIconic(IntPtr h);
 [DllImport("user32.dll")] static extern int GetWindowTextLength(IntPtr h);
 [DllImport("dwmapi.dll")] static extern int DwmGetWindowAttribute(IntPtr h,int attr,out uint value,int size);
 static uint Cloaked(IntPtr h){uint value;DwmGetWindowAttribute(h,14,out value,4);return value;}
 [DllImport("user32.dll")] static extern bool ShowWindow(IntPtr h,int n);
 [DllImport("user32.dll")] static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] static extern IntPtr GetForegroundWindow();
 [DllImport("user32.dll")] static extern int GetWindowLong(IntPtr h,int n);
 public static IntPtr Find(int pid,bool journal){IntPtr result=IntPtr.Zero;EnumWindows((h,p)=>{uint owner;GetWindowThreadProcessId(h,out owner);var text=new StringBuilder(256);GetWindowText(h,text,256);if(owner==pid&&text.ToString().Contains(journal?"日记与日历":"待办")){result=h;return false;}return true;},IntPtr.Zero);return result;}
 [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
 [DllImport("user32.dll")] static extern bool AttachThreadInput(uint a,uint b,bool attach);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr FindWindow(string name,string title);
 public static void FocusTaskbar(){FocusHandle(FindWindow("Shell_TrayWnd",null));}
 public static void HideWidget(int pid){ShowWindow(Find(pid,false),0);}
 public static void Minimize(int pid,bool journal){ShowWindow(Find(pid,journal),6);}
 [DllImport("user32.dll")] static extern void keybd_event(byte key,byte scan,uint flags,UIntPtr extra);
 public static void ShowDesktopKey(){keybd_event(0x5B,0,0,UIntPtr.Zero);keybd_event(0x44,0,0,UIntPtr.Zero);keybd_event(0x44,0,2,UIntPtr.Zero);keybd_event(0x5B,0,2,UIntPtr.Zero);}
 [StructLayout(LayoutKind.Sequential)] struct Point { public int X,Y; }
 [DllImport("user32.dll")] static extern IntPtr WindowFromPoint(Point p);
 [DllImport("user32.dll")] static extern IntPtr GetAncestor(IntPtr h,uint flags);
 static bool HitWidget(IntPtr h){Rect r;GetWindowRect(h,out r);return GetAncestor(WindowFromPoint(new Point{X=r.Left+25,Y=r.Top+70}),2)==h;}
 [StructLayout(LayoutKind.Sequential)] struct Rect { public int Left,Top,Right,Bottom; }
 [DllImport("user32.dll")] static extern bool GetWindowRect(IntPtr h,out Rect r);
 [DllImport("user32.dll")] static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] static extern void mouse_event(uint flags,uint x,uint y,uint data,UIntPtr extra);
 public static void Focus(int pid,bool journal){var h=Find(pid,journal);if(journal)FocusHandle(h);Rect r;GetWindowRect(h,out r);SetCursorPos(r.Left+25,r.Top+70);mouse_event(2,0,0,0,UIntPtr.Zero);mouse_event(4,0,0,0,UIntPtr.Zero);}
 static void FocusHandle(IntPtr h){if(h==IntPtr.Zero)throw new Exception("Test window missing");uint owner;uint foreground=GetWindowThreadProcessId(GetForegroundWindow(),out owner),current=GetCurrentThreadId();bool attached=foreground!=current&&AttachThreadInput(current,foreground,true);try{ShowWindow(h,9);SetForegroundWindow(h);}finally{if(attached)AttachThreadInput(current,foreground,false);}}
 [DllImport("user32.dll")] static extern bool ShowWindowAsync(IntPtr h,int mode);
 public static void MinimizeApplications(){EnumWindows((h,p)=>{if(IsWindowVisible(h)&&!IsIconic(h)&&Cloaked(h)==0&&(GetWindowLong(h,-16)&0xC00000)!=0&&(GetWindowLong(h,-20)&0x80)==0)ShowWindowAsync(h,6);return true;},IntPtr.Zero);}
 public static void Inventory(){EnumWindows((h,p)=>{if(IsWindowVisible(h)){var c=new StringBuilder(256);GetClassName(h,c,256);uint owner;GetWindowThreadProcessId(h,out owner);Console.WriteLine(c+" iconic="+IsIconic(h)+" cloaked="+Cloaked(h)+" titleLength="+GetWindowTextLength(h)+" style="+GetWindowLong(h,-16).ToString("X")+" ex="+GetWindowLong(h,-20).ToString("X")+" pid="+owner);}return true;},IntPtr.Zero);}
 public static string Status(int pid){var h=Find(pid,false);var f=GetForegroundWindow();var name=new StringBuilder(256);GetClassName(f,name,256);return "{\"hitWidget\":"+(HitWidget(h)?"true":"false")+",\"minimized\":"+(IsIconic(h)?"true":"false")+",\"cloaked\":"+Cloaked(h)+",\"visible\":"+(IsWindowVisible(h)?"true":"false")+",\"topmost\":"+((GetWindowLong(h,-20)&8)!=0?"true":"false")+",\"widgetFocused\":"+(h==f?"true":"false")+",\"foregroundClass\":\""+name+"\"}";}
}
'@
switch($Action){
 'desktop' {(New-Object -ComObject Shell.Application).MinimizeAll(); Start-Sleep -Milliseconds 500; [DesktopProbe]::MinimizeApplications(); Start-Sleep -Milliseconds 500}
 'winkeyd' {[DesktopProbe]::ShowDesktopKey(); Start-Sleep -Milliseconds 700}
 'minimize-journal' {[DesktopProbe]::Minimize($AppPid,$true); Start-Sleep -Milliseconds 400; [DesktopProbe]::MinimizeApplications(); Start-Sleep -Milliseconds 300}
 'hide-widget' {[DesktopProbe]::HideWidget($AppPid); Start-Sleep -Milliseconds 50}
 'minimize-widget' {[DesktopProbe]::Minimize($AppPid,$false); Start-Sleep -Milliseconds 50}
 'taskbar' {[DesktopProbe]::FocusTaskbar(); Start-Sleep -Milliseconds 300}
 'inventory' {[DesktopProbe]::Inventory()}
 'restore' {(New-Object -ComObject Shell.Application).UndoMinimizeAll(); Start-Sleep -Milliseconds 200}
 'journal' {[DesktopProbe]::Focus($AppPid,$true); Start-Sleep -Milliseconds 300}
 'widget' {[DesktopProbe]::Focus($AppPid,$false); Start-Sleep -Milliseconds 300}
}
[DesktopProbe]::Status($AppPid)
