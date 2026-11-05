#!/usr/bin/env python3
import os
import sys
import re
import json
from collections import defaultdict, Counter

# Reuse tokenizer from analyze_closures
from analyze_closures import analyze_file, scan_codebase

def detailed_analysis(target_dir):
    res = scan_codebase(target_dir)
    closures = res['app_closures']
    
    total = len(closures)
    pure_non_capturing = [c for c in closures if c.category == 'Pure_NonCapturing']
    pure_this_only = [c for c in closures if c.category == 'Pure_ThisCaptureOnly']
    pure_val_only = [c for c in closures if c.category == 'Pure_ValueCaptureOnly']
    pure_val_this = [c for c in closures if c.category == 'Pure_ValueAndThisCapture']
    impure_ref = [c for c in closures if c.category == 'Impure_ReferenceCapture']
    impure_global = [c for c in closures if c.category == 'Impure_GlobalAccess']
    
    # Analyze capture counts for pure value captures
    capture_count_dist = Counter()
    for c in pure_val_only:
        num_caps = len(c.val_uses) if c.kind == 'anonymous_function' else len(c.arrow_captured_vars)
        capture_count_dist[num_caps] += 1

    # Directory distribution
    dir_dist = defaultdict(lambda: {'total': 0, 'pure': 0, 'impure_ref': 0, 'non_cap': 0})
    for c in closures:
        rel_path = os.path.relpath(c.file_path, target_dir)
        top_part = rel_path.split(os.sep)
        if len(top_part) > 2 and top_part[0] == 'app':
            group = f"app/{top_part[1]}"
        elif len(top_part) > 1:
            group = top_part[0]
        else:
            group = "root"
            
        dir_dist[group]['total'] += 1
        if c.category == 'Impure_ReferenceCapture':
            dir_dist[group]['impure_ref'] += 1
        else:
            dir_dist[group]['pure'] += 1
        if c.category == 'Pure_NonCapturing':
            dir_dist[group]['non_cap'] += 1

    # Inspect impure patterns
    impure_samples = []
    for c in impure_ref[:30]:
        rel = os.path.relpath(c.file_path, target_dir)
        impure_samples.append({
            'file': rel,
            'line': c.line,
            'ref_uses': c.ref_uses,
            'val_uses': c.val_uses,
            'snippet': c.body_snippet[:80]
        })

    # Output JSON report
    report = {
        'total_files_scanned': res['app_file_count'],
        'total_closures': total,
        'pure_total': len(pure_non_capturing) + len(pure_this_only) + len(pure_val_only) + len(pure_val_this),
        'pure_pct': round((total - len(impure_ref) - len(impure_global)) / total * 100, 2) if total else 0,
        'impure_total': len(impure_ref) + len(impure_global),
        'impure_pct': round((len(impure_ref) + len(impure_global)) / total * 100, 2) if total else 0,
        'categories': {
            'Pure_NonCapturing': {
                'count': len(pure_non_capturing),
                'pct': round(len(pure_non_capturing) / total * 100, 2),
                'description': '0 captured variables, no $this. Zero allocation, pure raw Cranelift function pointer.'
            },
            'Pure_ThisCaptureOnly': {
                'count': len(pure_this_only),
                'pct': round(len(pure_this_only) / total * 100, 2),
                'description': 'Captures only $this by immutable object reference. No mutable outer local variables.'
            },
            'Pure_ValueCaptureOnly': {
                'count': len(pure_val_only),
                'pct': round(len(pure_val_only) / total * 100, 2),
                'description': 'Captures outer variables by immutable value-copy. No $this, no references.'
            },
            'Pure_ValueAndThisCapture': {
                'count': len(pure_val_this),
                'pct': round(len(pure_val_this) / total * 100, 2),
                'description': 'Captures $this and outer variables by value copy. No mutable references.'
            },
            'Impure_ReferenceCapture': {
                'count': len(impure_ref),
                'pct': round(len(impure_ref) / total * 100, 2),
                'description': 'Captures outer variables by reference use (&$var). Escaping mutable reference.'
            },
            'Impure_GlobalAccess': {
                'count': len(impure_global),
                'pct': round(len(impure_global) / total * 100, 2),
                'description': 'Accesses global $var or $GLOBALS.'
            }
        },
        'pure_val_capture_counts': dict(sorted(capture_count_dist.items())),
        'dir_breakdown': dict(sorted(dir_dist.items(), key=lambda x: x[1]['total'], reverse=True)),
        'impure_samples': impure_samples
    }
    
    with open('/home/yogesh/code/rhp/scripts/detailed_report.json', 'w') as f:
        json.dump(report, f, indent=2)

    print("Detailed report written to /home/yogesh/code/rhp/scripts/detailed_report.json")
    print(f"Total: {total}, Pure: {report['pure_total']} ({report['pure_pct']}%), Impure: {report['impure_total']} ({report['impure_pct']}%)")

if __name__ == '__main__':
    target = sys.argv[1] if len(sys.argv) > 1 else '/home/yogesh/code/client-app'
    detailed_analysis(target)
