/** PuTTY 0.83 utils/sessprep.c + utils/host_strduptrim.c semantics.
 * A user@ prefix overrides UserName. A single colon outside brackets is
 * discarded WITH its suffix, never used to override the separate port field. */
export function normalizeSshEndpoint<T extends {host:string;user:string;port:number}>(input:T):T {
  let host=input.host.replace(/^[ \t]+/,''),user=input.user;
  const at=host.lastIndexOf('@');if(at>=0){user=host.slice(0,at);host=host.slice(at+1);}
  let brackets=0;const colons:number[]=[];
  for(let i=0;i<host.length;i++){
    if(host[i]==='[')brackets++;
    else if(host[i]===']'&&brackets>0)brackets--;
    else if(host[i]===':'&&!brackets)colons.push(i);
  }
  if(colons.length===1)host=host.slice(0,colons[0]);
  host=host.replace(/[ \t]/g,'');
  const bracket=/^\[([a-fA-F0-9:]+(?:%[^\]]*)?)\]$/.exec(host);
  if(bracket&&(bracket[1].split('%')[0].match(/:/g)?.length??0)>1)host=bracket[1];
  return {...input,host,user};
}
export function normalizeSshInput(input:unknown):unknown {
  if(!input||typeof input!=='object'||Array.isArray(input))return input;
  const p=input as {host?:unknown;user?:unknown;port?:unknown};
  if(typeof p.host!=='string'||typeof p.user!=='string'||typeof p.port!=='number')return input;
  return normalizeSshEndpoint(input as {host:string;user:string;port:number});
}
