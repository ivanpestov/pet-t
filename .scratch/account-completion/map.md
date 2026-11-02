# Карта: account-completion

## Notes

Спека: `spec.md`. Тикеты: `issues/`. Один тикет, фронтир исчерпан.

## Decisions-so-far

- 01-account-completion (resolved): скрытый режим `t --complete <текущее>
  <предыдущее>` реализован в адаптерах (`src/adapters/completion.rs`,
  обнаружение в `src/adapters/args.rs`, вызов из `src/main.rs`), кандидаты —
  вывод `ledger accounts` с байтовой регистрозависимой префиксной
  фильтрацией в бинарнике; сбои дают пустой stdout и код 0, stderr дочернего
  процесса подавлен. Код выхода выбирается только в бинарнике (ADR-0001);
  протокол читает argv как сырые байты (`args_os`), паники на невалидном
  UTF-8 нет. Процессные тесты: `tests/account_completion.rs`. Влито в
  `feature/account-completion` (merge 5c7a5c7).

## Fog

Нет.
