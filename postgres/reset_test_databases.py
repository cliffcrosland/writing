#!/usr/bin/python3


import os
import subprocess
import sys


# We use 8 threads to execute tests. Each thread gets its own Postgres
# database to prevent data races.
#
# TODO(cliff): Move to an env variable?
NUM_TEST_DATABASES = 8


def run_command(args):
    print('command: {}'.format(" ".join(args)))
    process = subprocess.Popen(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    output, errs = process.communicate()
    output = output.decode('ascii').strip()
    errs = errs.decode('ascii').strip()
    if output:
        print('output: {}'.format(output))
    if errs:
        print('errs: {}'.format(errs))
    print()


def clear_test_database(database_name, database_num):
    print("== Clearing test database: {}{} ==".format(database_name, database_num))
    # Must be run one at a time. Cannot run in a single transaction block
    queries = [
        "DROP DATABASE {}{}".format(database_name, database_num),
        "DROP ROLE {}{}".format(database_name, database_num),
    ]
    for query in queries:
        args = [
            '/usr/bin/docker', 'exec',
            '-i', 'writing_postgres',
            'psql',
            '-U', 'postgres',
            '-c', query
        ]
        run_command(args)


def run_all_migrations(database_name, database_num):
    print("== [{}{}] Running all migrations ==".format(database_name, database_num))
    script_directory = os.path.dirname(os.path.realpath(__file__))
    migrations_directory = '{}/migrations'.format(script_directory)
    migration_file_names = sorted(os.listdir(migrations_directory))
    for migration_file_name in migration_file_names:
        if not migration_file_name.endswith("up.sh"):
            continue
        print('== [{}{}] Running migration {} =='.format(
            database_name, database_num, migration_file_name))
        migration_file_path = os.path.join(migrations_directory, migration_file_name)
        args = [migration_file_path, '_test{}'.format(database_num)]
        run_command(args)


def main():
    for i in range(NUM_TEST_DATABASES):
        database_num = i + 1
        clear_test_database('app_test', database_num)
        run_all_migrations('app_test', database_num)
    print("== Done! ==")


if __name__ == '__main__':
    main()

