using Shellfox.Native;
if(args is ["broker"]){using var broker=new LinuxBroker();await broker.Run();return 0;}
if(args is ["register","--ticket",var path,"--shell-pid",var pid]&&int.TryParse(pid,out var n)&&n>0)return await LinuxBroker.Register(path,n);
if(args is ["--self-test"]){
 var fields=new[]{"S","9"}.Concat(Enumerable.Repeat("0",17)).Append("123");var stat=Proc.Stat("8 (name with ) parens) "+string.Join(' ',fields));
 if(stat.Parent!=9||stat.Ticks!=123)throw new Exception("proc parser");
 if(Proc.Birth("00000000000000000000000000000001",1)==Proc.Birth("00000000000000000000000000000002",1))throw new Exception("boot identity");
 var id=Guid.NewGuid().ToString();var r=new Launch(id,id,id,"/tmp/Ω ' & ; % [data]","bash","/usr/bin/bash","unused","SHELLFOX:"+id+":"+id,null);var argv=LinuxBroker.Arguments(r,"/app/helper","/app/ticket.json","/app/bootstrap.bash");
 if(argv[4]!=r.Cwd||argv[5]!="--"||argv.Contains("-c"))throw new Exception("argv");
 if(Proc.Script(["node","--eval","pi/cli.js"])!=null)throw new Exception("script slot");
 Console.WriteLine("Linux helper self-test: 4 assertions passed; no launch/socket/window mutations.");return 0;
}
Console.Error.WriteLine("Shellfox.Linux broker | register --ticket <path> --shell-pid <pid>");return 2;
