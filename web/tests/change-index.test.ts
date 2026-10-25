import assert from 'node:assert/strict';
import {test} from 'node:test';
import {loadChangeIndex} from '../src/data/change-index';
import {FixtureSource} from '../src/data/fixture-source';
import {fixtures} from './fixture-data';

class ReconciledSource extends FixtureSource {
  provisional = true;
  reads = 0;
  async loadStatus() { return {provisional:this.provisional}; }
  async openChanges() { return []; }
  override async changes(project:string) {
    this.reads++;
    return super.changes(project);
  }
}
test('a captured provisional project switches to verified changes after reconciliation',async()=>{
  const source=new ReconciledSource(fixtures);
  const [recorded]=await source.projects();
  const project={...recorded,provisional:true};
  const first=await loadChangeIndex(source,project);
  assert.deepEqual(first.summaries,[]);
  assert.equal(source.reads,0,'A provisional first page does not load full change metadata');
  source.provisional=false;
  const full=await loadChangeIndex(source,project);
  assert.equal(full.summaries,undefined);
  assert(full.changes.length>0,'The stale project object must not hide reconciled changes');
  assert.equal(source.reads,1);
});
