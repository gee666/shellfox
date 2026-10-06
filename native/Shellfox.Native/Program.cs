using Shellfox.Native;

if(args is ["grant-foreground","--broker-pid",var brokerId] && int.TryParse(brokerId,out var brokerPid) && brokerPid>0){try{return Windows.GrantForeground(brokerPid)?0:1;}catch(Exception){return 1;}}
if(args is ["broker"]){ using var broker=new Broker(); await broker.Run(); return 0; }
if(args is ["register","--ticket",var ticket,"--shell-pid",var pid] && int.TryParse(pid,out var shellPid) && shellPid>0) return await Tickets.Register(ticket,shellPid);
Console.Error.WriteLine("Usage: Shellfox.Native.exe broker | register --ticket <path> --shell-pid <pid>");
return 2;
