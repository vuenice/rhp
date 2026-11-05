#!/usr/bin/env python3
import os
import sys
import re
import json
from collections import defaultdict

# PHP Tokenizer Regex
PHP_TOKEN_SPEC = [
    ('OPEN_TAG', r'<\?(?:php|=)?'),
    ('CLOSE_TAG', r'\?>'),
    ('COMMENT_MULTI', r'/\*[\s\S]*?\*/'),
    ('COMMENT_SINGLE', r'(?://|#)[^\r\n]*'),
    ('HEREDOC', r'<<<(?:\'([a-zA-Z_\x7f-\xff][a-zA-Z0-9_\x7f-\xff]*)\'|"([a-zA-Z_\x7f-\xff][a-zA-Z0-9_\x7f-\xff]*)"|([a-zA-Z_\x7f-\xff][a-zA-Z0-9_\x7f-\xff]*))\r?\n[\s\S]*?\r?\n\s*\1\2\3(?=;|\r?\n|,)'),
    ('STRING_SINGLE', r"'([^'\\]|\\.)*'"),
    ('STRING_DOUBLE', r'"([^"\\]|\\.)*"'),
    ('VARIABLE', r'\$[a-zA-Z_\x7f-\xff][a-zA-Z0-9_\x7f-\xff]*'),
    ('ARROW', r'=>'),
    ('NULLSAFE_OBJ_OP', r'\?->'),
    ('OBJ_OP', r'->'),
    ('PAAMAYIM_NEKUDOTAYIM', r'::'),
    ('ELLIPSIS', r'\.\.\.'),
    ('SYMBOL_MULTI', r'==|!=|<=|>=|===|!==|<=>|\+\+|--|\+=|-=|\*=|/=|%=|\.=|&&|\|\||\?\?|\?\?:'),
    ('IDENT', r'[a-zA-Z_\x7f-\xff][a-zA-Z0-9_\x7f-\xff]*'),
    ('SYMBOL', r'[{}()\[\],;:.&|!~+*/%^=<>\?@]'),
    ('WHITESPACE', r'[ \t\r\n]+'),
    ('MISC', r'.'),
]

TOKEN_RE = re.compile('|'.join(f'(?P<{name}>{pattern})' for name, pattern in PHP_TOKEN_SPEC))

def tokenize(code):
    tokens = []
    line = 1
    for mo in TOKEN_RE.finditer(code):
        kind = mo.lastgroup
        value = mo.group()
        if kind not in ('WHITESPACE', 'COMMENT_SINGLE', 'COMMENT_MULTI', 'OPEN_TAG', 'CLOSE_TAG'):
            tokens.append((kind, value, line))
        newlines = value.count('\n')
        if newlines:
            line += newlines
    return tokens

class ClosureInfo:
    def __init__(self, file_path, line, kind, is_static):
        self.file_path = file_path
        self.line = line
        self.kind = kind  # 'anonymous_function' or 'arrow_function'
        self.is_static = is_static
        self.params = []
        self.val_uses = []
        self.ref_uses = []
        self.uses_this = False
        self.uses_globals = False
        self.global_vars = []
        self.arrow_captured_vars = []
        self.body_snippet = ""
        self.category = ""

def analyze_file(file_path):
    try:
        with open(file_path, 'r', encoding='utf-8', errors='replace') as f:
            code = f.read()
    except Exception as e:
        return []

    tokens = tokenize(code)
    n = len(tokens)
    closures = []

    i = 0
    while i < n:
        tok_kind, tok_val, tok_line = tokens[i]

        # Check for closure: 'function' or 'fn'
        if tok_kind == 'IDENT' and tok_val in ('function', 'fn'):
            is_fn = (tok_val == 'fn')
            
            # Check if preceded by 'static'
            is_static = False
            prev_idx = i - 1
            if prev_idx >= 0 and tokens[prev_idx][0] == 'IDENT' and tokens[prev_idx][1] == 'static':
                # Check if it was a class method definition: e.g. 'public static function'
                p2_idx = prev_idx - 1
                if p2_idx >= 0 and tokens[p2_idx][0] == 'IDENT' and tokens[p2_idx][1] in ('public', 'protected', 'private', 'final', 'abstract'):
                    # This is a class method, not a closure
                    i += 1
                    continue
                is_static = True
            elif prev_idx >= 0 and tokens[prev_idx][0] == 'IDENT' and tokens[prev_idx][1] in ('public', 'protected', 'private', 'final', 'abstract'):
                # Regular method definition
                i += 1
                continue

            # Look ahead: after 'function', could have '&' (return by ref), then '(' or name
            j = i + 1
            returns_ref = False
            if j < n and tokens[j][0] == 'SYMBOL' and tokens[j][1] == '&':
                returns_ref = True
                j += 1

            if j >= n:
                i += 1
                continue

            # If the next token is an IDENT (and not '('), it's a named function: e.g. 'function foo('
            if not is_fn:
                if tokens[j][0] == 'IDENT':
                    # Named function
                    i += 1
                    continue
                if tokens[j][0] != 'SYMBOL' or tokens[j][1] != '(':
                    i += 1
                    continue
            else:
                # For 'fn', next must be '('
                if tokens[j][0] != 'SYMBOL' or tokens[j][1] != '(':
                    i += 1
                    continue

            # It is a closure!
            closure = ClosureInfo(file_path, tok_line, 'arrow_function' if is_fn else 'anonymous_function', is_static)
            
            # 1. Parse parameters inside '(' ... ')'
            # tokens[j] is '('
            depth = 1
            j += 1
            params = []
            while j < n and depth > 0:
                tk, tv, _ = tokens[j]
                if tk == 'SYMBOL' and tv == '(':
                    depth += 1
                elif tk == 'SYMBOL' and tv == ')':
                    depth -= 1
                    if depth == 0:
                        j += 1
                        break
                elif tk == 'VARIABLE' and depth == 1:
                    params.append(tv)
                j += 1
            closure.params = params

            if not is_fn:
                # Anonymous function: might have 'use (...)', return type ': type', and '{ ... }'
                # Check for 'use'
                val_uses = []
                ref_uses = []
                while j < n and not (tokens[j][0] == 'SYMBOL' and tokens[j][1] == '{'):
                    if tokens[j][0] == 'IDENT' and tokens[j][1] == 'use':
                        j += 1
                        if j < n and tokens[j][0] == 'SYMBOL' and tokens[j][1] == '(':
                            j += 1
                            depth = 1
                            expect_ref = False
                            while j < n and depth > 0:
                                tk, tv, _ = tokens[j]
                                if tk == 'SYMBOL' and tv == '(':
                                    depth += 1
                                elif tk == 'SYMBOL' and tv == ')':
                                    depth -= 1
                                    if depth == 0:
                                        j += 1
                                        break
                                elif tk == 'SYMBOL' and tv == '&' and depth == 1:
                                    expect_ref = True
                                elif tk == 'VARIABLE' and depth == 1:
                                    if expect_ref:
                                        ref_uses.append(tv)
                                        expect_ref = False
                                    else:
                                        val_uses.append(tv)
                                elif tk == 'SYMBOL' and tv == ',':
                                    expect_ref = False
                                j += 1
                    else:
                        j += 1

                closure.val_uses = val_uses
                closure.ref_uses = ref_uses

                # Now j should be at '{'
                if j < n and tokens[j][0] == 'SYMBOL' and tokens[j][1] == '{':
                    # Parse body up to matching '}'
                    body_start = j
                    body_depth = 1
                    j += 1
                    body_vars = []
                    uses_this = False
                    uses_globals = False
                    global_vars = []
                    
                    while j < n and body_depth > 0:
                        tk, tv, _ = tokens[j]
                        if tk == 'SYMBOL' and tv == '{':
                            body_depth += 1
                        elif tk == 'SYMBOL' and tv == '}':
                            body_depth -= 1
                            if body_depth == 0:
                                break
                        elif tk == 'VARIABLE':
                            if tv == '$this':
                                uses_this = True
                            elif tv == '$GLOBALS':
                                uses_globals = True
                            body_vars.append(tv)
                        elif tk == 'IDENT' and tv == 'global':
                            # global $x, $y;
                            uses_globals = True
                            gk = j + 1
                            while gk < n and not (tokens[gk][0] == 'SYMBOL' and tokens[gk][1] == ';'):
                                if tokens[gk][0] == 'VARIABLE':
                                    global_vars.append(tokens[gk][1])
                                gk += 1
                        j += 1

                    closure.uses_this = uses_this
                    closure.uses_globals = uses_globals
                    closure.global_vars = global_vars
                    body_snippet_tokens = [tv for _, tv, _ in tokens[body_start:min(j+1, body_start+15)]]
                    closure.body_snippet = " ".join(body_snippet_tokens)

            else:
                # Arrow function: 'fn(...) => <expr>'
                # Look for '=>'
                while j < n and not (tokens[j][0] == 'ARROW' and tokens[j][1] == '=>'):
                    j += 1
                if j < n and tokens[j][0] == 'ARROW':
                    j += 1 # past '=>'
                    # Expression extends until ';', or top-level ',', ')', ']', etc.
                    # Track delimiters: '(', '[', '{'
                    expr_vars = []
                    uses_this = False
                    expr_depth = 0
                    paren_depth = 0
                    bracket_depth = 0
                    brace_depth = 0
                    expr_start = j
                    
                    while j < n:
                        tk, tv, _ = tokens[j]
                        if tk == 'SYMBOL':
                            if tv == '(':
                                paren_depth += 1
                            elif tv == ')':
                                if paren_depth == 0:
                                    break
                                paren_depth -= 1
                            elif tv == '[':
                                bracket_depth += 1
                            elif tv == ']':
                                if bracket_depth == 0:
                                    break
                                bracket_depth -= 1
                            elif tv == '{':
                                brace_depth += 1
                            elif tv == '}':
                                if brace_depth == 0:
                                    break
                                brace_depth -= 1
                            elif tv == ';':
                                if paren_depth == 0 and bracket_depth == 0 and brace_depth == 0:
                                    break
                            elif tv == ',':
                                if paren_depth == 0 and bracket_depth == 0 and brace_depth == 0:
                                    break
                        elif tk == 'VARIABLE':
                            if tv == '$this':
                                uses_this = True
                            expr_vars.append(tv)
                        j += 1
                    
                    closure.uses_this = uses_this
                    # Any variable in expr_vars not in params is captured by value
                    captured = []
                    param_set = set(closure.params)
                    for v in expr_vars:
                        if v != '$this' and v not in param_set and v not in captured:
                            captured.append(v)
                    closure.arrow_captured_vars = captured
                    expr_snippet_tokens = [tv for _, tv, _ in tokens[expr_start:min(j, expr_start+10)]]
                    closure.body_snippet = "=> " + " ".join(expr_snippet_tokens)

            # Categorize closure
            if len(closure.ref_uses) > 0:
                closure.category = "Impure_ReferenceCapture"
            elif closure.uses_globals:
                closure.category = "Impure_GlobalAccess"
            elif closure.kind == 'arrow_function':
                if len(closure.arrow_captured_vars) == 0 and not closure.uses_this:
                    closure.category = "Pure_NonCapturing"
                elif closure.uses_this and len(closure.arrow_captured_vars) == 0:
                    closure.category = "Pure_ThisCaptureOnly"
                elif closure.uses_this and len(closure.arrow_captured_vars) > 0:
                    closure.category = "Pure_ValueAndThisCapture"
                else:
                    closure.category = "Pure_ValueCaptureOnly"
            else:
                # anonymous_function
                if len(closure.val_uses) == 0 and not closure.uses_this:
                    closure.category = "Pure_NonCapturing"
                elif closure.uses_this and len(closure.val_uses) == 0:
                    closure.category = "Pure_ThisCaptureOnly"
                elif closure.uses_this and len(closure.val_uses) > 0:
                    closure.category = "Pure_ValueAndThisCapture"
                else:
                    closure.category = "Pure_ValueCaptureOnly"

            closures.append(closure)
            i = j
            continue

        i += 1

    return closures

def scan_codebase(root_path):
    app_closures = []
    vendor_closures = []
    
    app_file_count = 0
    vendor_file_count = 0
    
    # Priority directories for Laravel app
    app_dirs = {'app', 'bootstrap', 'config', 'database', 'lang', 'resources', 'routes', 'tests', 'tools'}
    
    for dirpath, dirnames, filenames in os.walk(root_path):
        # Exclude .git, storage, node_modules
        rel_dir = os.path.relpath(dirpath, root_path)
        parts = rel_dir.split(os.sep)
        if any(p in ('.git', 'node_modules', 'storage') for p in parts):
            continue
            
        is_vendor = 'vendor' in parts
        
        for f in filenames:
            if f.endswith('.php'):
                full_path = os.path.join(dirpath, f)
                if is_vendor:
                    vendor_file_count += 1
                    # Can sample or scan vendor if needed
                else:
                    app_file_count += 1
                    file_closures = analyze_file(full_path)
                    app_closures.extend(file_closures)

    return {
        'app_file_count': app_file_count,
        'vendor_file_count': vendor_file_count,
        'app_closures': app_closures,
    }

if __name__ == '__main__':
    target_dir = sys.argv[1] if len(sys.argv) > 1 else '/home/yogesh/code/client-app'
    print(f"Scanning {target_dir}...")
    res = scan_codebase(target_dir)
    print(f"Scanned {res['app_file_count']} app PHP files.")
    closures = res['app_closures']
    print(f"Found {len(closures)} total closures in client-app application code.")
    
    cat_counts = defaultdict(int)
    kind_counts = defaultdict(int)
    static_counts = defaultdict(int)
    
    for c in closures:
        cat_counts[c.category] += 1
        kind_counts[c.kind] += 1
        if c.is_static:
            static_counts['static'] += 1
        else:
            static_counts['non-static'] += 1

    print("\n--- CLOSURE BREAKDOWN BY CATEGORY ---")
    for cat, count in sorted(cat_counts.items(), key=lambda x: x[1], reverse=True):
        pct = (count / len(closures)) * 100 if closures else 0
        print(f"  {cat:30s}: {count:5d} ({pct:5.1f}%)")

    print("\n--- CLOSURE BY KIND ---")
    for k, count in kind_counts.items():
        print(f"  {k:30s}: {count:5d}")

    print("\n--- STATIC DECLARATION ---")
    for s, count in static_counts.items():
        print(f"  {s:30s}: {count:5d}")
