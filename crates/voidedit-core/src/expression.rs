const MAX_DEPTH: u32 = 32;

struct Parser<'a> {
    text: &'a [u8],
    at: usize,
    value: f32,
    frame: f32,
    depth: u32,
}

pub fn evaluate(text: &str, value: f32, frame: f32) -> Option<f32> {
    let mut parser = Parser { text: text.as_bytes(), at: 0, value, frame, depth: 0 };
    let result = parser.sum()?;
    parser.skip();
    (parser.at == parser.text.len() && result.is_finite()).then_some(result)
}

impl Parser<'_> {
    fn skip(&mut self) {
        while self.text.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }

    fn eat(&mut self, symbol: u8) -> bool {
        self.skip();
        if self.text.get(self.at) == Some(&symbol) {
            self.at += 1;
            return true;
        }
        false
    }

    fn sum(&mut self) -> Option<f32> {
        let mut total = self.product()?;
        loop {
            if self.eat(b'+') {
                total += self.product()?;
            } else if self.eat(b'-') {
                total -= self.product()?;
            } else {
                return Some(total);
            }
        }
    }

    fn product(&mut self) -> Option<f32> {
        let mut total = self.atom()?;
        loop {
            if self.eat(b'*') {
                total *= self.atom()?;
            } else if self.eat(b'/') {
                total /= self.atom()?;
            } else if self.eat(b'%') {
                total %= self.atom()?;
            } else {
                return Some(total);
            }
        }
    }

    fn atom(&mut self) -> Option<f32> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return None;
        }
        let result = self.atom_inner();
        self.depth -= 1;
        result
    }

    fn atom_inner(&mut self) -> Option<f32> {
        if self.eat(b'-') {
            return Some(-self.atom()?);
        }
        if self.eat(b'(') {
            let inner = self.sum()?;
            return self.eat(b')').then_some(inner);
        }
        self.skip();
        let start = self.at;
        let first = *self.text.get(self.at)?;
        if first.is_ascii_digit() || first == b'.' {
            while self.text.get(self.at).is_some_and(|c| c.is_ascii_digit() || *c == b'.') {
                self.at += 1;
            }
            return std::str::from_utf8(&self.text[start..self.at]).ok()?.parse().ok();
        }
        while self.text.get(self.at).is_some_and(u8::is_ascii_alphabetic) {
            self.at += 1;
        }
        let name = std::str::from_utf8(&self.text[start..self.at]).ok()?;
        if !self.eat(b'(') {
            return match name {
                "value" => Some(self.value),
                "frame" => Some(self.frame),
                "time" => Some(self.frame / 30.0),
                "pi" => Some(std::f32::consts::PI),
                _ => None,
            };
        }
        let first = self.sum()?;
        let second = if self.eat(b',') { Some(self.sum()?) } else { None };
        if !self.eat(b')') {
            return None;
        }
        match (name, second) {
            ("sin", None) => Some(first.sin()),
            ("cos", None) => Some(first.cos()),
            ("abs", None) => Some(first.abs()),
            ("floor", None) => Some(first.floor()),
            ("min", Some(other)) => Some(first.min(other)),
            ("max", Some(other)) => Some(first.max(other)),
            ("wiggle", Some(amount)) => {
                let t = self.frame / 30.0 * first;
                Some(self.value + ((t * 1.3).sin() + (t * 2.7 + 1.1).sin() + (t * 4.1 + 2.3).sin()) / 3.0 * amount)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn math_and_variables_work() {
        assert_eq!(evaluate("value + 2 * 3", 4.0, 0.0), Some(10.0));
        assert_eq!(evaluate("max(frame, 2) - (1)", 0.0, 5.0), Some(4.0));
        assert_eq!(evaluate("-value", 3.0, 0.0), Some(-3.0));
    }

    #[test]
    fn bad_input_is_refused_not_crashing() {
        for text in ["", "1 +", "nope", "sin(", "1 / 0", "((((", &"(".repeat(5000), &"-".repeat(5000)] {
            assert_eq!(evaluate(text, 1.0, 1.0), None, "{}", &text[..text.len().min(8)]);
        }
    }
}
