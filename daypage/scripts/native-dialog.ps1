param([int]$AppPid,[string]$FilePath,[switch]$ConfirmRestore)
$ErrorActionPreference='Stop'
Add-Type @'
using System;using System.Text;using System.Collections.Generic;using System.Runtime.InteropServices;
public static class NativePicker {
 delegate bool EnumProc(IntPtr h,IntPtr p);
 [DllImport("user32.dll")] static extern bool EnumWindows(EnumProc p,IntPtr l);
 [DllImport("user32.dll")] static extern bool EnumChildWindows(IntPtr h,EnumProc p,IntPtr l);
 [DllImport("user32.dll")] static extern uint GetWindowThreadProcessId(IntPtr h,out uint p);
 [DllImport("user32.dll")] static extern int GetDlgCtrlID(IntPtr h);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetClassName(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern int GetWindowText(IntPtr h,StringBuilder s,int n);
 [DllImport("user32.dll",CharSet=CharSet.Unicode)] static extern IntPtr SendMessage(IntPtr h,uint m,IntPtr w,string s);
 [DllImport("user32.dll")] static extern IntPtr SendMessage(IntPtr h,uint m,IntPtr w,IntPtr l);
 static string Class(IntPtr h){var b=new StringBuilder(256);GetClassName(h,b,256);return b.ToString();}
 static string Text(IntPtr h){var b=new StringBuilder(512);GetWindowText(h,b,512);return b.ToString();}
 public static int Select(int pid,string path,bool confirm){
  int result=0;
  EnumWindows((window,p)=>{
   uint owner;GetWindowThreadProcessId(window,out owner);if(owner!=pid)return true;
   var children=new List<IntPtr>();EnumChildWindows(window,(h,l)=>{children.Add(h);return true;},IntPtr.Zero);
   if(confirm){foreach(var h in children)if(Class(h)=="Button"&&Text(h)=="恢复"){SendMessage(h,0xF5,IntPtr.Zero,IntPtr.Zero);result=1;return false;}}
   else {
    IntPtr field=IntPtr.Zero,accept=IntPtr.Zero;
    foreach(var h in children){if(Class(h)=="Edit"&&(GetDlgCtrlID(h)==1001||GetDlgCtrlID(h)==1148))field=h;if(Class(h)=="Button"&&GetDlgCtrlID(h)==1)accept=h;}
    if(field!=IntPtr.Zero&&accept!=IntPtr.Zero){SendMessage(field,0xC,IntPtr.Zero,path);SendMessage(accept,0xF5,IntPtr.Zero,IntPtr.Zero);result=1;return false;}
   }
   return true;
  },IntPtr.Zero);
  return result;
 }
 public static string Dump(int pid){var b=new StringBuilder();EnumWindows((window,p)=>{uint owner;GetWindowThreadProcessId(window,out owner);if(owner==pid){b.AppendLine(Class(window)+" "+Text(window));EnumChildWindows(window,(h,l)=>{if(Class(h)=="Edit"||Class(h)=="Button"||Class(h)=="#32770")b.AppendLine(Class(h)+" id="+GetDlgCtrlID(h)+" "+Text(h));return true;},IntPtr.Zero);}return true;},IntPtr.Zero);return b.ToString();}
}
'@
$deadline=[DateTime]::UtcNow.AddSeconds(30)
$filled=$false
while([DateTime]::UtcNow -lt $deadline){
 if([NativePicker]::Select($AppPid,$FilePath,$false)-eq 1){$filled=$true;break}
 Start-Sleep -Milliseconds 100
}
if(-not $filled){Write-Output ([NativePicker]::Dump($AppPid));throw 'Native file picker not found'}
if($ConfirmRestore){
 $deadline=[DateTime]::UtcNow.AddSeconds(15)
 while([DateTime]::UtcNow -lt $deadline){
  if([NativePicker]::Select($AppPid,'',$true)-eq 1){Write-Output 'Selected and confirmed';exit 0}
  Start-Sleep -Milliseconds 100
 }
 Write-Output ([NativePicker]::Dump($AppPid));throw 'Restore confirmation not found'
}
Write-Output 'Selected'
