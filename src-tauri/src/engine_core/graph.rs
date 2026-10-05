use std::collections::{HashMap, HashSet, VecDeque};
use super::{roblox_variants, Engine, StaticType, WordEntry};

impl Engine {
    pub fn from_text(text: &str) -> Result<Self, String> {
        let mut seen=HashSet::new();
        let mut words=Vec::new();
        let mut by_head:HashMap<char,Vec<usize>>=HashMap::new();
        let mut index_by_text=HashMap::new();
        for raw in text.lines() {
            let w=raw.trim();
            if w.is_empty() || w.chars().any(char::is_whitespace) || w.chars().count()<2 { continue; }
            if !seen.insert(w.to_owned()) { continue; }
            let head=w.chars().next().unwrap(); let tail=w.chars().last().unwrap();
            let idx=words.len();
            words.push(WordEntry{text:w.to_owned(),head,tail,len:w.chars().count()});
            by_head.entry(head).or_default().push(idx); index_by_text.insert(w.to_owned(),idx);
        }
        if words.len()<10 { return Err("사전 단어가 너무 적습니다.".into()); }

        let mut nodes=HashSet::new();
        for w in &words { nodes.insert(w.head); nodes.insert(w.tail); }
        let mut by_required=HashMap::new();
        for &node in &nodes {
            let mut list=Vec::new();
            for h in roblox_variants(node) { if let Some(v)=by_head.get(&h){ list.extend(v.iter().copied()); } }
            list.sort_unstable(); list.dedup(); by_required.insert(node,list);
        }
        let (static_types,static_depth,distinct_edges)=classify(&words,&by_required,&nodes);
        let replies_static=by_required.iter().map(|(&c,v)|(c,v.len())).collect();
        Ok(Self{words,by_required,index_by_text,static_types,static_depth,replies_static,distinct_edges,node_count:nodes.len()})
    }
    pub fn static_type(&self, ch: char) -> StaticType {
        self.static_types.get(&ch).copied().unwrap_or_else(|| if self.by_required.get(&ch).is_some_and(|v|!v.is_empty()){StaticType::Route}else{StaticType::Lose})
    }
}

fn classify(words:&[WordEntry], by:&HashMap<char,Vec<usize>>, nodes:&HashSet<char>) -> (HashMap<char,StaticType>,HashMap<char,u16>,usize) {
    let ns=nodes.iter().copied().collect::<Vec<_>>();
    let ids=ns.iter().enumerate().map(|(i,&c)|(c,i)).collect::<HashMap<_,_>>();
    let mut children=vec![Vec::<usize>::new();ns.len()]; let mut pred=vec![Vec::<usize>::new();ns.len()]; let mut edges=0;
    for (i,&n) in ns.iter().enumerate() {
        let mut seen=HashSet::new();
        if let Some(list)=by.get(&n){ for &wi in list { if let Some(&j)=ids.get(&words[wi].tail){ if seen.insert(j){children[i].push(j);pred[j].push(i);edges+=1;} } } }
    }
    let mut state=vec![0u8;ns.len()]; let mut depth=vec![0u16;ns.len()];
    let mut remain=children.iter().map(Vec::len).collect::<Vec<_>>(); let mut maxd=vec![0u16;ns.len()]; let mut q=VecDeque::new();
    for i in 0..ns.len(){ if children[i].is_empty(){state[i]=2;q.push_back(i);} }
    while let Some(n)=q.pop_front(){
        for &p in &pred[n] { if state[p]!=0{continue;} if state[n]==2 {state[p]=1;depth[p]=depth[n].saturating_add(1);q.push_back(p);} else {remain[p]=remain[p].saturating_sub(1);maxd[p]=maxd[p].max(depth[n]);if remain[p]==0{state[p]=2;depth[p]=maxd[p].saturating_add(1);q.push_back(p);}} }
    }
    let mut tm=HashMap::new(); let mut dm=HashMap::new();
    for (i,&c) in ns.iter().enumerate(){tm.insert(c,match state[i]{1=>StaticType::Win,2=>StaticType::Lose,_=>StaticType::Route});dm.insert(c,depth[i]);}
    (tm,dm,edges)
}
