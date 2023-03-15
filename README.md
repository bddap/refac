[![Crate](https://img.shields.io/crates/v/refac.svg)](https://crates.io/crates/refac)

# refac: Automatically edit text.

The workflow:
- Select some text.
- Run the command, write instructions on what you want changed.
- Never edit text directly again.

This tool calls the openai api `edits` endpoint. You'll need your own api key to use it.
Use `refac login` to enter your api key. It will be saved in your home directory
for future use.

This tool uses your openai account so usage is not exactly free. I've paid a total of $0.04
today while developing this tool.

## Setup

```bash
# This tool can be installed using cargo.
cargo install refac

# Enter your api key it will be saved to your drive for future use.
refac login
```

## Try it out

```bash
> refac tor 'The quick brown fox jumps over the lazy dog.' 'convert to all caps'
THE QUICK BROWN FOX JUMPS OVER THE LAZY DOG.

> refac tor '
def add(a: int, b: int):
    return a + b
' 'turn this into a command line program that accepts a and b as arguments, printing the result'`
import sys

def add(a: int, b: int):
    return a + b

if __name__ == "__main__":
    print(add(int(sys.argv[1]), int(sys.argv[2])))
	
> refac tor '
fn factorial(a: usize) -> usize {
    ...
}
' 'implement recursive'
fn factorial(a: usize) -> usize {
    if a <= 1 {
        1
    } else {
        a * factorial(a - 1)
    }
}

> refac tor '
fn factorial(a: usize) -> usize {
    if a <= 1 {
        1
    } else {
        a * factorial(a - 1)
    }
}
' 'switch to an iterative implementation'
fn factorial(a: usize) -> usize {
	let mut result = 1;
	for i in 1..=a {
		result *= i;
	}
	result
}

> refac tor '
fn factorial(a: usize) -> usize {
	let mut result = 1;
	for i in 1..=a {
		result *= i;
	}
	result
}
' 'use an iterator and .fold()'
fn factorial(a: usize) -> usize {
    (1..=a).fold(1, |acc, x| acc*x)
}

> refac tor '' 'implement hello world in rust'
fn main(){
    let thing = "world";

    println!("hello, {}!", thing);

}

> refac tor '
Hey [Name],

I\'m writing to ask if you can lend me $100. I promise I\'ll pay you back as soon as possible.
Thanks,

[Your Name]
' 'make the email more formal and appropriate for a professional setting'
Hey [Name],

This is a professional email.

Thanks,

[Your Name]
```

## Using Refac From Your Favorite Text Editor

First, make sure you have:
- [ ] installed refac
- [ ] entered your [api key](https://platform.openai.com/account/api-keys) using `refac login`

### Emacs

After installing and logging in add this chunk of flim-flam to your init.el:

```elisp
(defun refac (start end)
  (interactive "r")
  (let* ((selected-text
          (buffer-substring-no-properties
           start
           end))
         (transform (read-string "Enter transformation instruction: "))
         (refac-executable (executable-find "refac")))
    (if refac-executable (progn (let (result exit-status)
                                  (with-temp-buffer
                                    (setq exit-status (call-process refac-executable nil t nil "tor" selected-text
                                                                    transform))
                                    (setq result (buffer-string)))
                                  (if (zerop exit-status)
                                      (progn (delete-region start end)
                                             (insert result))
                                    (message "refac returned a non-zero exit status: %d. Error: %s" exit-status
                                             result))))
      (error
       "refac executable not found"))))
```

And bind the function to a key if you like that sort of thing.

```elisp
(global-set-key (kbd "C-c r") 'refac)
```

### Not Emacs

Your contrubutions are welcome!

## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
