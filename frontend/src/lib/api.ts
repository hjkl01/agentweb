export type ApiErrorBody = { error: { code: string; message: string } };
export class ApiError extends Error { status?:number; code?:string; path:string; constructor(message:string,path:string,status?:number,code?:string){super(message);this.name='ApiError';this.path=path;this.status=status;this.code=code;} }
function requestOptions(options?:RequestInit):RequestInit{return{credentials:'include',headers:{'Content-Type':'application/json',...(options?.headers||{})},...options};}
export async function api<T=any>(path:string,options?:RequestInit):Promise<T>{
 const url='/api'+path;let response:Response;try{response=await fetch(url,requestOptions(options));}catch(error){const message=error instanceof Error?error.message:String(error);throw new ApiError(`无法连接后端：${message}`,path);}
 const text=await response.text();let data:any={};if(text){try{data=JSON.parse(text)}catch{data={error:{code:'INVALID_RESPONSE',message:text.slice(0,500)}}}}
 if(response.status===401&&path!=='/auth/login')window.dispatchEvent(new CustomEvent('agentweb:auth-expired'));
 if(!response.ok){const error=data?.error;const message=typeof error==='object'?error.message:(typeof error==='string'?error:(data?.message||response.statusText));const code=typeof error==='object'?error.code:undefined;throw new ApiError(message,path,response.status,code);}return data as T;
}
export function fileUrl(sessionId:string,path:string):string{return`/sessions/${sessionId}/file/${path.split('/').map(encodeURIComponent).join('/')}`;}
