import { createAgent, type BaseAgent, type KnownPublicKeys } from 'ssh2';
export function agentEndpoints(platform=process.platform,env=process.env):string[]{
 return platform==='win32'?['pageant','\\\\.\\pipe\\openssh-ssh-agent']:env.SSH_AUTH_SOCK?[env.SSH_AUTH_SOCK]:[];
}
export async function availableAgent(platform=process.platform,env=process.env):Promise<{agent:BaseAgent;keys:KnownPublicKeys;endpoint:string}|null>{
 let empty:{agent:BaseAgent;keys:KnownPublicKeys;endpoint:string}|null=null;
 for(const endpoint of agentEndpoints(platform,env)){
  const agent=createAgent(endpoint);
  const keys=await new Promise<KnownPublicKeys|null>(resolve=>{
   const timer=setTimeout(()=>resolve(null),1500);
   agent.getIdentities((error,keys)=>{clearTimeout(timer);resolve(error?null:keys??[]);});
  });
  if(keys?.length)return {agent,keys,endpoint};
  if(keys)empty={agent,keys,endpoint};
 }
 return empty;
}
