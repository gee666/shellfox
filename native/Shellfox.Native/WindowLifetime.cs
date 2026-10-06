using System.Runtime.InteropServices;

namespace Shellfox.Native;

// A dedicated message-pump thread is required for out-of-context WinEvent delivery.
public sealed class WindowLifetime : IDisposable {
 delegate void Callback(nint hook,uint evt,nint hwnd,int objectId,int childId,uint thread,uint time);
 [DllImport("user32.dll")] static extern nint SetWinEventHook(uint min,uint max,nint module,Callback callback,uint pid,uint thread,uint flags);
 [DllImport("user32.dll")] static extern bool UnhookWinEvent(nint hook);
 [StructLayout(LayoutKind.Sequential)] struct Message { public nint Hwnd;public uint Id;public nuint WParam;public nint LParam;public uint Time;public int X;public int Y;public uint Private; }
 [DllImport("user32.dll")] static extern int GetMessage(out Message message,nint hwnd,uint min,uint max);
 [DllImport("user32.dll")] static extern bool PeekMessage(out Message message,nint hwnd,uint min,uint max,uint remove);
 [DllImport("user32.dll")] static extern bool PostThreadMessage(uint id,uint message,nuint w,nint l);
 [DllImport("kernel32.dll")] static extern uint GetCurrentThreadId();
 readonly Thread thread;
 readonly Callback callback;
 readonly ManualResetEventSlim ready=new();
 uint threadId;
 nint hook;
 public WindowLifetime(Action<string> destroyed){
  callback=(_,evt,hwnd,objectId,childId,_,_)=>{ if(evt==0x8001 && hwnd!=0 && objectId==0 && childId==0)destroyed(hwnd.ToInt64().ToString()); };
  thread=new Thread(()=>{
   threadId=GetCurrentThreadId(); PeekMessage(out _,0,0,0,0);
   hook=SetWinEventHook(0x8001,0x8001,0,callback,0,0,0);
   ready.Set();
   if(hook==0)return;
   while(GetMessage(out _,0,0,0)>0){}
   UnhookWinEvent(hook);
  }){IsBackground=true,Name="Shellfox HWND destruction watcher"};
  thread.Start();ready.Wait(TimeSpan.FromSeconds(2));
  if(hook==0)throw new NativeError("NATIVE_UNAVAILABLE","Could not install native window lifetime watcher.");
 }
 public void Dispose(){PostThreadMessage(threadId,0x0012,0,0);thread.Join(TimeSpan.FromSeconds(1));ready.Dispose();}
}
