// check-upstream.sh links ./upstream to the real PiMate protocol in a temporary directory.
import {
  encodeLine, formatPairingLink, parsePairingLink, PROTOCOL_VERSION,
  type AppMessage, type HostMessage, type ServerMessage, type InstanceSnapshot,
  type AuthFlow, type SubagentChild, type Request, type Response,
} from './upstream/index.ts';

const snapshot: InstanceSnapshot = {
  protocol: PROTOCOL_VERSION, instanceId: 'pi-1', pid: 123, cwd: '/project', origin: 'pc',
  abducoSession: null, startedAt: 1740000000123, session: null, status: 'idle', model: null,
  thinking: null, pendingUI: [], pcPrompt: null, seq: 8,
  context: { tokens: null, contextWindow: 128000, percent: 4.125 },
  quotas: [{source:"codex", observedAt:1740000000123, windows:[{minutes:300, usedPercent:0}, {minutes:15.5, usedPercent:2.75, resetsAt:1740000000123.5}]}],
};
const app: AppMessage[] = [
  { type: 'auth', token: 'fixture-only', protocol: PROTOCOL_VERSION, app: {version:'1',platform:'ios'} },
  { type: 'rc', instanceId: 'pi-1', request: { id: '1', method: 'prompt', expectInstanceId: 'pi-1',
    params: { text: '日本語\n🙂', images: [{type:'image', mimeType:'image/png', data:'YQ=='}] } } },
  { type: 'github', requestId: '2', instanceId: 'pi-1', query: {kind:'pr', number:4} },
];
const host: HostMessage[] = [
  { type: 'subscribed', instanceId: 'pi-1', snapshot, history: {session:null,items:[
    {kind:'assistant', entryId:'msg-1', text:'ok', toolCalls:[], usage:{input:32,output:1,cost:0.000025}},
    {kind:'tool_result',entryId:'msg-2',text:'',toolCallId:'t',toolName:'read',isError:false,details:null,imageCount:1},
  ]} },
  { type: 'spawn_result', requestId: '3', ok:false, code:'spawn_failed' },
  { type: 'github_result',requestId:'4',ok:true,data:{kind:'list',repo:{name:'owner/repo',url:'https://example.org'},issues:[],prs:[]} },
  { type: 'host_restart_result',requestId:'5',ok:true },
];
const child: SubagentChild = {id:'0',index:0,agent:'worker',state:'failed',hasTranscript:false,actions:[],exitCode:-1};
const flow:AuthFlow = {id:'test-flow',provider:'anthropic',method:'oauth',revision:2,expiresAt:1740000000123,status:'input',prompt:{id:'p',kind:'code'}};
const begin = {id:'6',method:'auth_begin',params:{provider:'anthropic',method:'oauth'}} satisfies Request<'auth_begin'>;
const receipt = {type:'response',id:'6',ok:true,result:{flow}} satisfies Response<'auth_begin'>;
const server:ServerMessage[] = [ receipt,
 {type:'response',id:null,ok:false,error:{code:'bad_request',data:null}},
 {type:'event',seq:9007199254740991,sessionId:null,instanceId:'pi-1',event:{type:'subagent_run',run:{id:'r',backend:'pi-subagents',background:true,mode:'single',state:'failed',actions:[],children:[child]}}},
];
const pairing = parsePairingLink(formatPairingLink({address:'[::1]:47800',hostName:'Mac & mini',code:'fixtureonly123456',hostId:'h'}));
console.log(JSON.stringify(JSON.parse(encodeLine({app,host,server,begin,receipt,pairing})),null,2));
