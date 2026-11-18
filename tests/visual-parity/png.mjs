import {inflateSync,deflateSync,crc32} from 'node:zlib';

const signature=Buffer.from('89504e470d0a1a0a','hex');
const limit=4096;
function requireThat(condition,message) {if(!condition)throw new Error(message);}
export function decodePng(bytes) {
  requireThat(Buffer.isBuffer(bytes)&&bytes.subarray(0,8).equals(signature),'Expected PNG');
  let offset=8,header,ended=false;
  const idat=[];
  while(offset<bytes.length) {
    requireThat(offset+12<=bytes.length,'Truncated PNG chunk');
    const size=bytes.readUInt32BE(offset);
    requireThat(size<=128*1024*1024&&offset+12+size<=bytes.length,'Invalid PNG chunk size');
    const type=bytes.toString('ascii',offset+4,offset+8);
    const data=bytes.subarray(offset+8,offset+8+size);
    requireThat(crc32(bytes.subarray(offset+4,offset+8+size))===bytes.readUInt32BE(offset+8+size),'Invalid PNG CRC');
    if(type==='IHDR') {requireThat(!header&&size===13,'Invalid PNG header');header=data;}
    else if(type==='IDAT')idat.push(data);
    else if(type==='IEND'){ended=true;offset+=12+size;break;}
    offset+=12+size;
  }
  requireThat(header&&ended&&offset===bytes.length&&idat.length,'Incomplete PNG');
  const width=header.readUInt32BE(0),height=header.readUInt32BE(4),type=header[9];
  requireThat(width>0&&height>0&&width<=limit&&height<=limit,'Unsupported PNG dimensions');
  requireThat(header[8]===8&&[2,6].includes(type)&&header[10]===0&&header[11]===0&&header[12]===0,'Only non-interlaced 8-bit RGB/RGBA PNGs are supported');
  const channels=type===6?4:3,rowSize=width*channels;
  const filtered=inflateSync(Buffer.concat(idat),{maxOutputLength:(rowSize+1)*height});
  requireThat(filtered.length===(rowSize+1)*height,'Unexpected PNG raster length');
  const raw=Buffer.alloc(rowSize*height),pixels=Buffer.alloc(width*height*4);
  for(let y=0;y<height;y++) {
    const filter=filtered[y*(rowSize+1)];
    requireThat(filter<=4,'Unsupported PNG filter');
    for(let x=0;x<rowSize;x++) {
      const a=x>=channels?raw[y*rowSize+x-channels]:0,b=y?raw[(y-1)*rowSize+x]:0,c=y&&x>=channels?raw[(y-1)*rowSize+x-channels]:0;
      let predictor=0;
      if(filter===1)predictor=a;
      else if(filter===2)predictor=b;
      else if(filter===3)predictor=Math.floor((a+b)/2);
      else if(filter===4){const p=a+b-c,pa=Math.abs(p-a),pb=Math.abs(p-b),pc=Math.abs(p-c);predictor=pa<=pb&&pa<=pc?a:pb<=pc?b:c;}
      raw[y*rowSize+x]=(filtered[y*(rowSize+1)+1+x]+predictor)&255;
    }
    for(let x=0;x<width;x++) {
      const from=y*rowSize+x*channels,to=(y*width+x)*4;
      pixels[to]=raw[from];pixels[to+1]=raw[from+1];pixels[to+2]=raw[from+2];pixels[to+3]=channels===4?raw[from+3]:255;
    }
  }
  return {width,height,pixels};
}
function chunk(type,data) {
  const name=Buffer.from(type),length=Buffer.alloc(4),checksum=Buffer.alloc(4);
  length.writeUInt32BE(data.length);checksum.writeUInt32BE(crc32(Buffer.concat([name,data])));
  return Buffer.concat([length,name,data,checksum]);
}
export function encodePng({width,height,pixels}) {
  requireThat(width>0&&height>0&&width<=limit&&height<=limit&&pixels.length===width*height*4,'Invalid raster');
  const header=Buffer.alloc(13);header.writeUInt32BE(width);header.writeUInt32BE(height,4);header[8]=8;header[9]=6;
  const rows=Buffer.alloc((width*4+1)*height);
  for(let y=0;y<height;y++)pixels.copy(rows,y*(width*4+1)+1,y*width*4,(y+1)*width*4);
  return Buffer.concat([signature,chunk('IHDR',header),chunk('IDAT',deflateSync(rows)),chunk('IEND',Buffer.alloc(0))]);
}
export function compareImages(expected,actual) {
  const width=Math.max(expected.width,actual.width),height=Math.max(expected.height,actual.height);
  const diff=Buffer.alloc(width*height*4);
  let changedPixels=0,maxChannelDelta=0,minX=width,minY=height,maxX=-1,maxY=-1;
  for(let y=0;y<height;y++)for(let x=0;x<width;x++) {
    const inside=x<expected.width&&y<expected.height&&x<actual.width&&y<actual.height;
    const a=(y*expected.width+x)*4,b=(y*actual.width+x)*4;
    let changed=!inside,delta=inside?0:255;
    if(inside)for(let c=0;c<4;c++)delta=Math.max(delta,Math.abs(expected.pixels[a+c]-actual.pixels[b+c]));
    changed ||= delta!==0;
    const offset=(y*width+x)*4;
    if(changed){changedPixels++;maxChannelDelta=Math.max(maxChannelDelta,delta);minX=Math.min(minX,x);minY=Math.min(minY,y);maxX=Math.max(maxX,x);maxY=Math.max(maxY,y);diff.set([255,0,inside?0:255,255],offset);}
    else {const grey=Math.round((expected.pixels[a]+expected.pixels[a+1]+expected.pixels[a+2])/6);diff.set([grey,grey,grey,255],offset);}
  }
  return {equal:changedPixels===0,dimensionsEqual:expected.width===actual.width&&expected.height===actual.height,
    changedPixels,totalPixels:width*height,maxChannelDelta,bounds:changedPixels?{x:minX,y:minY,width:maxX-minX+1,height:maxY-minY+1}:null,
    diff:{width,height,pixels:diff}};
}
