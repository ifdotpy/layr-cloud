import assert from 'node:assert/strict';
import {test} from 'node:test';
import {epochRoute} from '../src/data/epochs';
const root={format:2,project:'project',recipient:'root recipient'};
const epoch='01a12290-0000-7000-8000-000000000008', id='01a12290-0000-7000-8000-000000000009';
const object=`keys/${epoch}-${id}.json`;
const record={id,op:'project.key',project:root.project,data:{project:root.project,epoch,parent:'root',parent_recipient:root.recipient,recipient:'new recipient'}};
test('epoch routing chooses current paths and preserves root as a history source',()=>{
 assert.deepEqual(epochRoute(root,{}),{epoch:'root',prefix:'',history:[]});
 assert.deepEqual(epochRoute(root,{[object]:JSON.stringify({r:record})}),{epoch,prefix:`project/epochs/${epoch}/`,history:['root']});
 for(const value of [{...record,project:'foreign'},{...record,data:{...record.data,parent:epoch}},{...record,data:{...record.data,parent_recipient:'foreign'}}])
   assert.throws(()=>epochRoute(root,{[object]:JSON.stringify({r:value})}));
 const fork='01a12290-0000-7000-8000-000000000007';
 assert.throws(()=>epochRoute(root,{[object]:JSON.stringify({r:record}),[`keys/${fork}-${id}.json`]:JSON.stringify({r:{...record,data:{...record.data,epoch:fork}}})}),/ambiguous/);
});
