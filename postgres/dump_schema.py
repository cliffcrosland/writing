#!/usr/bin/python3


import os
import re
import subprocess
import sys


def add_definition(definition, definitions):
    d = definition
    if d[0].find('CREATE TABLE') != -1:
        d[0] = re.sub(r'CREATE TABLE public.', 'CREATE TABLE ', d[0])
        table_name = d[0].strip().split(' ')[2]
        definitions.setdefault(table_name, {})['table'] = '\n'.join(d)
    elif d[0].find('CREATE') != -1 and d[0].find('INDEX') != -1:
        d[0] = re.sub(r' ON public.', ' ON ', d[0])
        words = d[0].split(' ')
        index_name = words[words.index('INDEX') + 1]
        table_name = words[words.index('ON') + 1]
        definitions.setdefault(table_name, {}).setdefault('indexes', []).append((index_name, '\n'.join(d)))
    else:
        print('Unknown d type: {}'.format('\n'.join(definition)))
        exit(1)


def parse_definitions(lines):
    definition = []
    definitions = {}
    seeking = True
    for line in lines:
        if seeking:
            if line.find('CREATE') == -1:
                continue
            elif line.find('TABLE') != -1 or line.find('INDEX') != -1:
                seeking = False
                definition.append(line)
        else:
            if line.strip():
                definition.append(line)
            else:
                add_definition(definition, definitions)
                seeking = True
                definition.clear()
    if definition:
        add_definition(definition, definitions)
    for table_name in definitions:
        indexes = definitions[table_name].get('indexes')
        if indexes:
            indexes.sort()
    return definitions


def dump_schema_to_file(schema_file_path):
    print('Dumping schema for app postgres database...')
    args = [
        '/usr/bin/docker', 'exec',
        '-i', 'writing_postgres',
        'pg_dump',
        '-U', 'app',
        '-d', 'app',
        '--schema-only'
    ]
    process = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    output, errs = process.communicate()
    output = output.decode('ascii')
    lines = []
    error_occurred = False
    for line in output.split('\n'):
        if not error_occurred and line.find('pg_dump: error:') != -1:
            error_occurred = True
        lines.append(line)

    if error_occurred:
        print('An error occurred while dumping the schema.')
        print('\n'.join(lines))
        exit(1)

    definitions = parse_definitions(lines)
    keys = list(definitions.keys())
    keys.sort()

    with open(schema_file_path, 'w') as f:
        first = True
        for key in keys:
            if first:
                first = False
            else:
                f.write("\n\n")
            table = definitions[key]['table']
            f.write(table)
            f.write("\n")
            for (_, index) in definitions[key].get('indexes', []):
                f.write(index)
                f.write("\n")
    print('Done.')
    print('Database schema written to:')
    print(schema_file_path)


def main():
    script_directory = os.path.dirname(os.path.realpath(__file__))
    schema_file_path = '{}/schema.sql'.format(script_directory)
    dump_schema_to_file(schema_file_path)


if __name__ == '__main__':
    main()

