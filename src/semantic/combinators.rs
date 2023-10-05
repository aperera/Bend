use crate::ast::{hvm_lang::DefNames, Definition, DefinitionBook, Name, Rule, Term};

/// Replaces closed Terms (i.e. without free variables) with a Ref to the extracted term
/// Precondition: Vars must have been sanitized
impl DefinitionBook {
  pub fn detach_combinators(&mut self) {
    let mut comb = self.register_combinators();

    for def in self.defs.iter_mut() {
      for rule in def.rules.iter_mut() {
        rule.body.abstract_lambdas(&self.def_names);
      }
    }

    self.defs.append(&mut comb);
  }
}

impl Term {
  pub fn abstract_lambdas(&mut self, names: &DefNames) {
    pub fn go(term: &mut Term, depth: usize, names: &DefNames) {
      match term {
        Term::Lam { nam: Some(_), bod } => {
          if !(depth == 0 && bod.is_simple()) {
            term.abstract_lambda(names)
          }
        }
        Term::Lam { nam: _, bod } => go(bod, depth + 1, names),
        Term::Chn { nam: _, bod } => go(bod, depth + 1, names),
        Term::Let { nam: _, val, nxt } => {
          go(val, depth + 1, names);
          go(nxt, depth + 1, names);
        }
        Term::App { fun, arg } => {
          go(fun, depth + 1, names);
          go(arg, depth + 1, names);
        }
        Term::Dup { fst: _, snd: _, val, nxt } => {
          go(val, depth + 1, names);
          go(nxt, depth + 1, names);
        }
        Term::Sup { .. } => todo!(),
        Term::Var { .. } => {}
        Term::Lnk { .. } => {}
        Term::Ref { .. } => {}
        Term::Era => {}
      }
    }

    go(self, 0, names)
  }

  pub fn is_simple(&self) -> bool {
    match self {
      Self::Var { .. } => true,
      Self::Lam { nam: _, bod } => bod.is_simple(),
      Self::Chn { nam: _, bod } => bod.is_simple(),
      Self::Lnk { .. } => true,
      Self::Let { .. } => false,
      Self::Ref { .. } => true,
      Self::App { fun, arg } => fun.is_simple() && arg.is_simple(),
      Self::Dup { .. } => false,
      Self::Sup { .. } => false,
      Self::Era => true,
    }
  }
}

#[derive(Debug)]
pub enum Combinator {
  K,
  I,
  B,
  C,
  S,
  B_,
  C_,
  S_,
}

#[derive(Debug)]
pub enum AbsTerm {
  Term(Term),
  Comb(Combinator),
  App(Box<AbsTerm>, Box<AbsTerm>),
}

impl From<Term> for AbsTerm {
  fn from(value: Term) -> Self {
    Self::Term(value)
  }
}

impl From<Combinator> for AbsTerm {
  fn from(value: Combinator) -> Self {
    Self::Comb(value)
  }
}

impl AbsTerm {
  pub fn call(called: Combinator, args: impl IntoIterator<Item = AbsTerm>) -> Self {
    args.into_iter().fold(called.into(), |acc, arg| AbsTerm::App(Box::new(acc), Box::new(arg)))
  }

  pub fn to_string(&self, names: &DefNames) -> String {
    match self {
      Self::Term(term) => term.to_string(names),
      Self::Comb(comb) => format!("{:?}", comb),
      Self::App(k, args) => {
        format!("({} {})", k.to_string(names), args.to_string(names))
      }
    }
  }

  pub fn to_term(self, names: &DefNames) -> Term {
    match self {
      Self::Term(term) => term,
      Self::Comb(c) => comb_ref(&format!("_{:?}", c), names),
      Self::App(k, args) => Term::app(k.to_term(names), args.to_term(names)),
    }
  }

  pub fn abstract_by(self, name: &str) -> Self {
    match self {
      Self::Term(term) => term.abstract_by(name),
      Self::Comb(comb) => Self::call(Combinator::K, [Self::Comb(comb)]),

      Self::App(fun, box Self::Term(Term::Var { nam: Name(n) })) if n == name && !fun.occours_check(name) => {
        *fun
      }

      _ if !self.occours_check(name) => AbsTerm::call(Combinator::K, vec![self]),

      Self::App(box Self::App(fun, arg), arg2) if !fun.occours_check(name) => {
        match (arg.occours_check(name), arg2.occours_check(name)) {
          (false, true) => AbsTerm::call(Combinator::B_, vec![*fun, *arg, arg2.abstract_by(name)]),
          (true, false) => AbsTerm::call(Combinator::C_, vec![*fun, arg.abstract_by(name), *arg2]),
          (true, true) => {
            AbsTerm::call(Combinator::S_, vec![*fun, arg.abstract_by(name), arg2.abstract_by(name)])
          }
          (false, false) => unreachable!(),
        }
      }

      Self::App(fun, arg) => match (fun.occours_check(name), arg.occours_check(name)) {
        (false, true) => AbsTerm::call(Combinator::B, vec![*fun, arg.abstract_by(name)]),
        (true, false) => AbsTerm::call(Combinator::C, vec![fun.abstract_by(name), *arg]),
        (true, true) => AbsTerm::call(Combinator::S, vec![fun.abstract_by(name), arg.abstract_by(name)]),
        (false, false) => unreachable!(),
      },
    }
  }

  pub fn occours_check(&self, name: &str) -> bool {
    match self {
      Self::Term(term) => term.occours_check(name),
      Self::Comb(_) => false,
      Self::App(fun, arg) => fun.occours_check(name) || arg.occours_check(name),
    }
  }

  pub fn reduce(&mut self) {
    use Combinator as C;

    match self {
      Self::App(
        box Self::App(box Self::App(box Self::Comb(C::C_), box Self::Comb(c)), box Self::Comb(C::I)),
        box Self::Comb(C::I),
      ) => match c {
        C::C_ => *self = Self::Comb(C::C), // (C_ C_ I I) => C
        C::S_ => *self = Self::Comb(C::S), // (C_ S_ I I) => S
        _ => {}
      },

      Self::App(
        box Self::App(box Self::Comb(C::C_), box Self::App(box Self::Comb(C::C), box Self::Comb(C::I))),
        box Self::Comb(C::I),
      ) => *self = Self::Comb(C::I), // (C_ (C_ I) I) => I

      Self::App(f, a) => {
        f.reduce();
        a.reduce();
      }
      _ => {}
    }
  }
}

fn comb_ref(name: &str, names: &DefNames) -> Term {
  let def_id = names.def_id(&Name::new(name)).unwrap();
  Term::Ref { def_id }
}

impl Term {
  pub fn abstract_lambda(&mut self, names: &DefNames) {
    let extracted = std::mem::replace(self, Term::Era);
    let Self::Lam { nam: Some(name), bod } = extracted else { panic!() };

    *self = if bod.channel_check(&name, false) {
      Self::Lam { nam: Some(name), bod }
    } else {
      let mut abstracted = bod.abstract_by(&name);
      abstracted.reduce();
      abstracted.to_term(names)
    };
  }

  /// abcdef algorithm - pp. 42–67 of the second volume of Combinatory Logic
  pub fn abstract_by(self, name: &str) -> AbsTerm {
    use AbsTerm as A;
    use Combinator as C;

    match self {
      // Especial case
      Self::Lam { nam: Some(n), bod } => {
        if bod.channel_check(&n, false) {
          A::Term(Self::Lam { nam: Some(n), bod })
        } else {
          bod.abstract_by(&n).abstract_by(name)
        }
      }

      // Especial case
      Self::Lam { nam: None, bod } => A::call(C::K, vec![bod.abstract_by(name)]),

      // (a)
      _ if !self.occours_check(name) => A::call(C::K, vec![self.into()]),

      // (b)
      Self::Var { .. } => C::I.into(),

      // Especial case
      Self::App { fun: box Self::Lam { nam: Some(n), bod }, arg } => {
        if bod.channel_check(&n, false) {
          A::App(Box::new(A::Term(Self::Lam { nam: Some(n), bod })), Box::new(arg.abstract_by(name)))
        } else {
          A::App(Box::new(bod.abstract_by(&n)), Box::new((*arg).into())).abstract_by(name)
        }
      }

      // Especial case
      Self::App { fun, arg: box Self::Lam { nam: Some(n), bod } } => {
        if bod.channel_check(&n, false) {
          A::App(Box::new(fun.abstract_by(name)), Box::new(A::Term(Self::Lam { nam: Some(n), bod })))
        } else {
          A::App(Box::new((*fun).into()), Box::new(bod.abstract_by(&n))).abstract_by(name)
        }
      }

      // (c)
      Self::App { fun, arg } if arg.is_var(name) && !fun.occours_check(name) => (*fun).into(),

      Self::App { fun: box Self::App { fun, arg }, arg: arg2 } if !fun.occours_check(name) => {
        match (arg.occours_check(name), arg2.occours_check(name)) {
          (false, true) => A::call(C::B_, vec![(*fun).into(), (*arg).into(), arg2.abstract_by(name)]),
          (true, false) => A::call(C::C_, vec![(*fun).into(), arg.abstract_by(name), (*arg2).into()]),
          (true, true) => A::call(C::S_, vec![(*fun).into(), arg.abstract_by(name), arg2.abstract_by(name)]),
          (false, false) => unreachable!(),
        }
      }

      // (d)
      Self::App { fun, arg } if !fun.occours_check(name) && arg.occours_check(name) => {
        A::call(C::B, vec![(*fun).into(), arg.abstract_by(name)])
      }

      // (e)
      Self::App { fun, arg } if fun.occours_check(name) && !arg.occours_check(name) => {
        A::call(C::C, vec![fun.abstract_by(name), (*arg).into()])
      }

      // (f)
      Self::App { fun, arg } if fun.occours_check(name) && arg.occours_check(name) => {
        A::call(C::S, vec![fun.abstract_by(name), arg.abstract_by(name)])
      }

      Self::Dup { fst: Some(nam), snd: Some(s), val, mut nxt } => {
        nxt.subst(&s, &Term::Var { nam: nam.clone() });
        A::App(Box::new(nxt.abstract_if_occours(&nam)), Box::new((*val).into())).abstract_by(name)
      }

      Self::Dup { fst: Some(nam), snd: None, val, nxt }
      | Self::Dup { fst: None, snd: Some(nam), val, nxt } => {
        A::App(Box::new(nxt.abstract_if_occours(&nam)), Box::new((*val).into())).abstract_by(name)
      }

      Self::Dup { fst: None, snd: None, val: _, nxt } => nxt.abstract_by(name),

      Self::Let { nam, val, nxt } => {
        A::App(Box::new(nxt.abstract_if_occours(&nam)), Box::new((*val).into())).abstract_by(name)
      }

      Self::Chn { .. } => unimplemented!(),

      Self::Sup { .. } => todo!(),

      Self::App { .. } => unreachable!(),
      Self::Ref { .. } => unreachable!(),
      Self::Lnk { .. } => unreachable!(),
      Self::Era => unreachable!(),
    }
  }

  pub fn occours_check(&self, name: &str) -> bool {
    match self {
      Self::Var { nam: Name(n) } => n == name,
      Self::Lam { nam, bod } => !nam.as_ref().is_some_and(|Name(n)| n == name) && bod.occours_check(name),
      Self::Chn { nam: _, bod } => bod.occours_check(name),
      Self::App { fun, arg } => fun.occours_check(name) || arg.occours_check(name),
      Self::Sup { fst, snd } => fst.occours_check(name) || snd.occours_check(name),
      Self::Let { nam: Name(n), val, nxt } => {
        val.occours_check(name) || (n != name && nxt.occours_check(name))
      }
      Self::Dup { fst, snd, val, nxt } => {
        val.occours_check(name)
          || (!fst.as_ref().is_some_and(|Name(n)| n == name)
            && !snd.as_ref().is_some_and(|Name(n)| n == name)
            && nxt.occours_check(name))
      }
      Self::Lnk { .. } => false,
      Self::Ref { .. } => false,
      Self::Era => false,
    }
  }

  fn channel_check(&self, name: &str, inside_chn: bool) -> bool {
    match self {
      Self::Var { nam: Name(n) } => inside_chn && n == name,
      Self::Lam { nam, bod } => {
        !nam.as_ref().is_some_and(|Name(n)| n == name) && bod.channel_check(name, inside_chn)
      }
      Self::Chn { nam: _, bod } => bod.channel_check(name, true),
      Self::App { fun, arg } => fun.channel_check(name, inside_chn) || arg.channel_check(name, inside_chn),
      Self::Sup { fst, snd } => fst.channel_check(name, inside_chn) || snd.channel_check(name, inside_chn),
      Self::Let { nam: Name(n), val, nxt } => {
        val.channel_check(name, inside_chn) || (n != name && nxt.channel_check(name, inside_chn))
      }

      Self::Dup { fst, snd, val, nxt } => {
        if val.occours_check(name) {
          if let Some(f) = fst {
            if nxt.channel_check(f, inside_chn) {
              return true;
            };
          }
          if let Some(s) = snd {
            if nxt.channel_check(s, inside_chn) {
              return true;
            };
          }
        }

        val.channel_check(name, inside_chn)
          || (!fst.as_ref().is_some_and(|Name(n)| n == name)
            && !snd.as_ref().is_some_and(|Name(n)| n == name)
            && nxt.channel_check(name, inside_chn))
      }

      Self::Lnk { .. } => false,
      Self::Ref { .. } => false,
      Self::Era => false,
    }
  }

  fn abstract_if_occours(self, name: &str) -> AbsTerm {
    match self.occours_check(name) {
      true => self.abstract_by(name),
      false => self.into(),
    }
  }

  fn is_var(&self, name: &str) -> bool {
    matches!(self, Term::Var { nam: Name(n) } if n == name)
  }

  fn lam(name: Option<&str>, body: Self) -> Self {
    Self::Lam { nam: name.map(Name::new), bod: Box::new(body) }
  }

  fn app(fun: Self, arg: Self) -> Self {
    Self::App { fun: Box::new(fun), arg: Box::new(arg) }
  }

  fn var(name: &str) -> Self {
    Self::Var { nam: Name::new(name) }
  }
}

impl DefinitionBook {
  pub fn register_combinators(&mut self) -> Vec<Definition> {
    let mut combinators = Vec::new();

    combinators.push(self.register("_K", Term::lam(Some("x"), Term::lam(Some("y"), Term::var("x")))));
    combinators.push(self.register("_I", Term::lam(Some("x"), Term::var("x"))));

    let xyz_lambda = |body| Term::lam(Some("x"), Term::lam(Some("y"), Term::lam(Some("z"), body)));
    let dxyz_lambda = |body| {
      Term::lam(
        Some("d"),
        xyz_lambda({
          let Term::App { fun, arg } = body else { panic!() };
          Term::App { fun: Box::new(Term::App { fun: Box::new(Term::var("d")), arg: fun }), arg }
        }),
      )
    };

    let b_body = Term::app(Term::var("x"), Term::app(Term::var("y"), Term::var("z")));

    combinators.push(self.register("_B", xyz_lambda(b_body.clone())));
    combinators.push(self.register("_B_", dxyz_lambda(b_body)));

    let c_body = Term::app(Term::app(Term::var("x"), Term::var("z")), Term::var("y"));

    combinators.push(self.register("_C", xyz_lambda(c_body.clone())));
    combinators.push(self.register("_C_", dxyz_lambda(c_body)));

    let s_body =
      Term::app(Term::app(Term::var("x"), Term::var("z")), Term::app(Term::var("y"), Term::var("z")));

    combinators.push(self.register("_S", xyz_lambda(s_body.clone())));
    combinators.push(self.register("_S_", dxyz_lambda(s_body)));

    combinators
  }

  pub fn register(&mut self, name: &str, body: Term) -> Definition {
    let def_id = self.def_names.insert(Name::new(name));
    Definition { def_id, rules: vec![Rule { def_id, pats: Vec::new(), body }] }
  }
}

#[cfg(test)]
mod tests {
  use crate::ast::{DefinitionBook, Term};

  #[test]
  fn test() {
    let mut book = DefinitionBook::new();
    book.register_combinators();

    let mut test_case = Term::lam(
      Some("f"),
      Term::lam(
        Some("g"),
        Term::lam(
          Some("x"),
          Term::app(
            Term::app(Term::var("some_var"), Term::app(Term::var("f"), Term::var("x"))),
            Term::app(Term::var("g"), Term::var("x")),
          ),
        ),
      ),
    );

    println!("Term:\n{}\n", test_case.to_string(&book.def_names));
    test_case.abstract_lambdas(&book.def_names);
    println!("Result:\n{}\n", test_case.to_string(&book.def_names));
  }
}
