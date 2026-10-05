use std::cmp::Ordering;
use std::collections::{HashMap,HashSet};
use std::time::Instant;
use super::{roblox_variants,AnalysisOutput,CandidateAnalysis,Engine,PlayMode,SearchConfig,StaticType,WordEntry};

const MATE:i32=30_000;
const INF:i32=32_000;

struct Ctx{
    used:HashSet<usize>,
    hash:u64,
    nodes:u64,
    tt:HashMap<(char,u8,u64),i32>,
    beam:usize,
    started:Instant,
    time_limit_ms:u64,
    node_limit:u64,
    stopped:bool,
}
impl Ctx{
    fn new(used:HashSet<usize>,cfg:SearchConfig)->Self{
        let hash=used.iter().fold(0,|a,&i|a^z(i));
        Self{
            used,hash,nodes:0,tt:HashMap::new(),beam:cfg.beam_width.max(4),
            started:Instant::now(),time_limit_ms:cfg.time_limit_ms.max(50),
            node_limit:cfg.node_limit.max(1_000),stopped:false,
        }
    }
    fn push(&mut self,i:usize){if self.used.insert(i){self.hash^=z(i)}}
    fn pop(&mut self,i:usize){if self.used.remove(&i){self.hash^=z(i)}}
    fn exhausted(&mut self)->bool{
        if self.stopped{return true}
        if self.nodes>=self.node_limit || self.started.elapsed().as_millis() as u64>=self.time_limit_ms{
            self.stopped=true;
            true
        }else{false}
    }
 #[test]
 fn excludes_previous_round_words(){
     let e=Engine::from_text("가나\n가다\n나다\n다라\n라가\n나가\n다가\n가라\n라마\n마가\n").unwrap();
     let excluded=vec!["가나".to_string()];
     let r=e.analyze_with_required(&[],&excluded,Some('가'),PlayMode::Neutral,SearchConfig{depth:2,beam_width:8,time_limit_ms:100,node_limit:10_000},10).unwrap();
     assert!(r.candidates.iter().all(|c|c.word!="가나"));
 }
}

impl Engine {
 pub fn analyze(&self,history:&[String],mode:PlayMode,cfg:SearchConfig,max:usize)->Result<AnalysisOutput,String>{
  self.analyze_with_required(history,&[],None,mode,cfg,max)
 }

 pub fn analyze_with_required(&self,history:&[String],excluded:&[String],initial_required:Option<char>,mode:PlayMode,cfg:SearchConfig,max:usize)->Result<AnalysisOutput,String>{
  let started=Instant::now();
  let (used,history_required,mut warnings)=self.prepare(history,excluded)?;
  let required=if history.is_empty(){initial_required.or(history_required)}else{history_required};
  let pos=required.map(|c|self.static_type(c));
  let mut ctx=Ctx::new(used,cfg);

  let mut moves=match required{
      Some(c)=>self.reps(c,&ctx.used),
      None=>self.first_reps(&ctx.used)
  };
  if moves.is_empty(){
      return Ok(AnalysisOutput{
          required,position_static:pos,candidates:vec![],best_word:None,pv:vec![],
          nodes:0,elapsed_ms:started.elapsed().as_millis() as u64,warnings
      })
  }

  self.pre_sort(&mut moves,mode);
  let root_cap=(max.max(8)*2).clamp(20,64).min(moves.len());
  moves.truncate(root_cap);

  let mut out=Vec::new();
  for i in moves{
      let w=&self.words[i];
      ctx.push(i);
      let replies=self.count(w.tail,&ctx.used);
      let st=self.static_type(w.tail);
      let opponent_attacks=if replies==0{0}else{self.count_static_attacks(w.tail,&ctx.used)};

      let score=if replies==0{
          MATE-1
      }else if mode==PlayMode::Neutral && (st!=StaticType::Route || opponent_attacks>0){
          -self.leaf(w.tail,replies)-8_000-(opponent_attacks.min(100) as i32)*120
      }else if cfg.depth<=1 || ctx.exhausted(){
          -self.leaf(w.tail,replies)
      }else{
          -self.negamax(w.tail,cfg.depth-1,-INF,INF,&mut ctx,1)
      };

      ctx.pop(i);
      let status=if replies==0{"finish"}else{match st{
          StaticType::Lose=>"win",
          StaticType::Route=>"neutral",
          StaticType::Win=>"danger"
      }};
      out.push((i,CandidateAnalysis{
          word:w.text.clone(),tail:w.tail,opponent_static:st,status,score,replies,
          opponent_attacks,
          neutrality:neutrality(st,replies,opponent_attacks,score),
          safety:safety(score),
          forced:score.abs()>=MATE-(cfg.depth as i32+4),
      }));

      if ctx.exhausted() && out.len()>=12{break}
  }

  self.final_sort(&mut out,mode);
  out.truncate(max.max(1));
  let best=out.first().map(|x|x.0);
  let best_word=out.first().map(|x|x.1.word.clone());

  if ctx.stopped{
      warnings.push(format!("CPU 보호를 위해 탐색을 {} ms / {} nodes 예산에서 중단하고 휴리스틱 평가를 사용했습니다.",cfg.time_limit_ms,cfg.node_limit));
  }

  let pv_cfg=SearchConfig{
      depth:cfg.depth.min(5),
      beam_width:cfg.beam_width.min(12),
      time_limit_ms:cfg.time_limit_ms.min(450),
      node_limit:cfg.node_limit.min(60_000),
  };
  let pv=best.map(|i|self.pv(i,&ctx.used,pv_cfg)).unwrap_or_default();

  Ok(AnalysisOutput{
      required,position_static:pos,candidates:out.into_iter().map(|x|x.1).collect(),
      best_word,pv,nodes:ctx.nodes,elapsed_ms:started.elapsed().as_millis() as u64,warnings
  })
 }

 fn prepare(&self,h:&[String],excluded:&[String])->Result<(HashSet<usize>,Option<char>,Vec<String>),String>{
  let mut used=HashSet::new();
  for raw in excluded {
      let w=raw.trim();
      if let Some(&i)=self.index_by_text.get(w){used.insert(i);}
  }
  let mut seen=HashSet::new();
  let mut req=None;
  let mut warn=vec![];
  for (turn,raw) in h.iter().enumerate(){
      let w=raw.trim();
      if w.chars().count()<2{return Err(format!("{}번째 단어가 너무 짧습니다: {}",turn+1,w))}
      if !seen.insert(w.to_string()){return Err(format!("현재 라운드 중복 단어입니다: {w}"))}
      if let Some(&i)=self.index_by_text.get(w){
          if used.contains(&i){return Err(format!("이전 라운드에서 이미 사용한 단어입니다: {w}"))}
      }
      let head=w.chars().next().unwrap();
      if let Some(r)=req{
          if !roblox_variants(r).contains(&head){
              return Err(format!("연결되지 않는 단어입니다: '{}' 뒤에 '{}'는 올 수 없습니다.",h[turn-1],w))
          }
      }
      if let Some(&i)=self.index_by_text.get(w){used.insert(i);}
      else{warn.push(format!("'{w}'는 현재 로컬 사전에 없지만 기록으로 유지했습니다."));}
      req=w.chars().last();
  }
  Ok((used,req,warn))
 }

 fn first_reps(&self,used:&HashSet<usize>)->Vec<usize>{
  let mut m=HashMap::new();
  for (i,w) in self.words.iter().enumerate(){
      if used.contains(&i){continue}
      m.entry(w.tail).and_modify(|b:&mut usize|if rep_cmp(w,&self.words[*b])==Ordering::Less{*b=i}).or_insert(i);
  }
  m.into_values().collect()
 }

 fn reps(&self,r:char,used:&HashSet<usize>)->Vec<usize>{
  let mut m=HashMap::new();
  if let Some(v)=self.by_required.get(&r){
      for &i in v{
          if used.contains(&i){continue}
          let w=&self.words[i];
          m.entry(w.tail).and_modify(|b:&mut usize|if rep_cmp(w,&self.words[*b])==Ordering::Less{*b=i}).or_insert(i);
      }
  }
  m.into_values().collect()
 }

 fn count(&self,r:char,used:&HashSet<usize>)->usize{
  self.by_required.get(&r).map(|v|v.iter().filter(|i|!used.contains(i)).count()).unwrap_or(0)
 }

 fn count_static_attacks(&self,r:char,used:&HashSet<usize>)->usize{
  self.by_required.get(&r).map(|v|{
      v.iter().filter(|&&i|{
          if used.contains(&i){return false}
          let tail=self.words[i].tail;
          self.static_type(tail)==StaticType::Lose || self.replies_static.get(&tail).copied().unwrap_or(0)==0
      }).count()
  }).unwrap_or(0)
 }

 fn leaf(&self,r:char,n:usize)->i32{
  let d=*self.static_depth.get(&r).unwrap_or(&20) as i32;
  match self.static_type(r){
      StaticType::Win=>1800-d.min(60)*12+branch(n),
      StaticType::Lose=>-1800+d.min(60)*12+branch(n)/4,
      StaticType::Route=>branch(n)
  }
 }

 fn ordered(&self,r:char,used:&HashSet<usize>,beam:usize)->Vec<usize>{
  let mut v=self.reps(r,used);
  v.sort_by(|&a,&b|{
      let x=&self.words[a];
      let y=&self.words[b];
      hard_pr(self.static_type(x.tail)).cmp(&hard_pr(self.static_type(y.tail)))
          .then_with(||self.replies_static.get(&x.tail).unwrap_or(&0).cmp(self.replies_static.get(&y.tail).unwrap_or(&0)))
          .then_with(||x.len.cmp(&y.len))
          .then_with(||x.text.cmp(&y.text))
  });
  v.truncate(beam.max(4));
  v
 }

 fn negamax(&self,r:char,d:u8,mut a:i32,b:i32,ctx:&mut Ctx,ply:i32)->i32{
  ctx.nodes+=1;
  let n=self.count(r,&ctx.used);
  if n==0{return -MATE+ply}
  if d==0 || ctx.exhausted(){return self.leaf(r,n)}
  if let Some(&v)=ctx.tt.get(&(r,d,ctx.hash)){return v}

  let mv=self.ordered(r,&ctx.used,ctx.beam);
  if mv.is_empty(){return -MATE+ply}
  let mut best=-INF;
  for i in mv{
      if ctx.exhausted(){break}
      let t=self.words[i].tail;
      ctx.push(i);
      let s=-self.negamax(t,d-1,-b,-a,ctx,ply+1);
      ctx.pop(i);
      best=best.max(s);
      a=a.max(s);
      if a>=b{break}
  }
  if best==-INF{best=self.leaf(r,n)}
  if !ctx.stopped{ctx.tt.insert((r,d,ctx.hash),best);}
  best
 }

 fn pre_sort(&self,v:&mut[usize],mode:PlayMode){
  v.sort_by(|&a,&b|{
      let x=&self.words[a];
      let y=&self.words[b];
      let tx=self.static_type(x.tail);
      let ty=self.static_type(y.tail);
      let rx=*self.replies_static.get(&x.tail).unwrap_or(&0);
      let ry=*self.replies_static.get(&y.tail).unwrap_or(&0);
      match mode{
          PlayMode::Neutral=>neutral_type_pr(tx).cmp(&neutral_type_pr(ty)).then_with(||ry.cmp(&rx)),
          PlayMode::Random=>z(a).cmp(&z(b)),
          _=>hard_pr(tx).cmp(&hard_pr(ty)).then_with(||rx.cmp(&ry))
      }
  })
 }

 fn final_sort(&self,v:&mut[(usize,CandidateAnalysis)],mode:PlayMode){
  v.sort_by(|(_,a),(_,b)|match mode{
      PlayMode::Neutral=>
          a.opponent_attacks.cmp(&b.opponent_attacks)
          .then_with(||neutral_type_pr(a.opponent_static).cmp(&neutral_type_pr(b.opponent_static)))
          .then_with(||b.replies.cmp(&a.replies))
          .then_with(||b.neutrality.cmp(&a.neutrality))
          .then_with(||b.safety.cmp(&a.safety)),
      PlayMode::Safe=>b.safety.cmp(&a.safety).then_with(||a.opponent_attacks.cmp(&b.opponent_attacks)).then_with(||b.score.cmp(&a.score)),
      PlayMode::Attack=>attack_pr(a).cmp(&attack_pr(b)).then_with(||b.score.cmp(&a.score)),
      PlayMode::Random=>word_hash(&a.word).cmp(&word_hash(&b.word)),
      PlayMode::Hard=>b.score.cmp(&a.score).then_with(||b.safety.cmp(&a.safety)).then_with(||a.replies.cmp(&b.replies))
  })
 }

 fn pv(&self,first:usize,used:&HashSet<usize>,cfg:SearchConfig)->Vec<String>{
  let mut ctx=Ctx::new(used.clone(),cfg);
  let mut out=vec![];
  let mut i=first;
  for ply in 0..cfg.depth.max(1){
      if ctx.exhausted(){break}
      ctx.push(i);
      out.push(self.words[i].text.clone());
      if ply+1>=cfg.depth{break}
      let r=self.words[i].tail;
      let moves=self.ordered(r,&ctx.used,ctx.beam);
      if moves.is_empty(){break}
      let mut best=moves[0];
      let mut bs=-INF;
      for c in moves{
          if ctx.exhausted(){break}
          let t=self.words[c].tail;
          ctx.push(c);
          let rem=cfg.depth.saturating_sub(ply+2);
          let count=self.count(t,&ctx.used);
          let s=if count==0{MATE-ply as i32}
          else if rem==0 || ctx.exhausted(){-self.leaf(t,count)}
          else{-self.negamax(t,rem,-INF,INF,&mut ctx,ply as i32+1)};
          ctx.pop(c);
          if s>bs{bs=s;best=c}
      }
      i=best;
  }
  out
 }
}

fn rep_cmp(a:&WordEntry,b:&WordEntry)->Ordering{
    natural(a).cmp(&natural(b)).then_with(||a.len.cmp(&b.len)).then_with(||a.text.cmp(&b.text))
}
fn natural(w:&WordEntry)->usize{match w.len{2..=5=>0,6..=8=>1,9..=12=>2,_=>4}}
fn hard_pr(t:StaticType)->u8{match t{StaticType::Lose=>0,StaticType::Route=>1,StaticType::Win=>2}}
fn neutral_type_pr(t:StaticType)->u8{match t{StaticType::Route=>0,StaticType::Lose=>1,StaticType::Win=>2}}
fn branch(n:usize)->i32{(((n.min(400)+1)as f64).ln()*70.0-160.0)as i32}
fn safety(s:i32)->u8{if s>=MATE-100{100}else if s<=-MATE+100{0}else{(50+s/50).clamp(0,100)as u8}}
fn neutrality(t:StaticType,r:usize,attacks:usize,s:i32)->u8{
    if r==0{return 0}
    let mut n:i32=match t{StaticType::Route=>72,StaticType::Lose=>42,StaticType::Win=>12};
    n+=((r.min(220) as f64+1.0).ln()*5.0) as i32;
    if attacks==0{n+=22}else{n-=(attacks.min(20) as i32)*8}
    if s< -2000{n-=30}else if s< -800{n-=12}else if s>2000{n-=10}else if s.abs()<=600{n+=8}
    n.clamp(0,100)as u8
}
fn attack_pr(c:&CandidateAnalysis)->(u8,usize,std::cmp::Reverse<i32>){
    (match c.status{"finish"=>0,"win"=>1,"neutral"=>2,_=>3},c.replies,std::cmp::Reverse(c.score))
}
fn word_hash(w:&str)->u64{
    w.as_bytes().iter().fold(0x9E3779B97F4A7C15u64,|a,b|(a^*b as u64).wrapping_mul(0xBF58476D1CE4E5B9))
}
fn z(i:usize)->u64{
    let mut x=(i as u64).wrapping_add(0x9E3779B97F4A7C15);
    x=(x^(x>>30)).wrapping_mul(0xBF58476D1CE4E5B9);
    x=(x^(x>>27)).wrapping_mul(0x94D049BB133111EB);
    x^(x>>31)
}

#[cfg(test)]
mod tests{
 use super::*;
 #[test]
 fn duplicate(){
     let e=Engine::from_text("가나\n나가\n나다\n다라\n라가\n가다\n다가\n가라\n라마\n마가\n").unwrap();
     assert!(e.analyze(&["가나".into(),"나가".into(),"가나".into()],PlayMode::Hard,SearchConfig::default(),10).is_err());
 }
 #[test]
 fn initial_required_filters_first_move(){
     let e=Engine::from_text("가나\n가다\n나다\n다라\n라가\n나가\n다가\n라마\n마가\n가라\n").unwrap();
     let r=e.analyze_with_required(&[],&[],Some('가'),PlayMode::Neutral,SearchConfig{depth:2,beam_width:8,time_limit_ms:100,node_limit:10_000},10).unwrap();
     assert!(r.candidates.iter().all(|c|c.word.starts_with('가')));
 }
}
